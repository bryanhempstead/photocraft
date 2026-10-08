//! Photoshop action sets (`.atn`, and the `Actions Palette.psp` preferences file, which holds the
//! same body).
//!
//! Layout (Adobe's public "Photoshop File Formats Specification", Actions file format):
//!
//! ```text
//! u32 version (16)          [.psp: u32 set count follows; .atn holds one set]
//! set:    unicode name, u8 expanded, u32 action count
//! action: u16 function key, u8 shift, u8 command, u16 colour, unicode name, u8 expanded,
//!         u32 step count
//! step:   u8 expanded, u8 enabled, u8 with dialog, u8 dialog options,
//!         4-byte id type ('TEXT' → u32 length + ASCII event id, 'long' → 4-char code),
//!         u32 length + ASCII step name, i32 descriptor flag (-1 → an unversioned descriptor)
//! ```
//!
//! Descriptors have no length prefix, so a step whose descriptor can't be read ends the parse:
//! everything read so far is kept and [`AtnFile::warnings`] says where it stopped.

use crate::descriptor::Descriptor;
use crate::error::{PsdError, Result};
use crate::io::{Reader, read_unicode_units};

/// Most sets, actions per set and steps per action accepted (fuzz safety).
pub const MAX_ITEMS: u32 = 100_000;

/// One recorded step.
#[derive(Debug, Clone, PartialEq)]
pub struct AtnStep {
    /// Event id (`gaussianBlur`, `Mk  `, `copyToLayer`…).
    pub event: String,
    /// The name Photoshop shows in the Actions panel (`Gaussian Blur`, `Make`…).
    pub name: String,
    /// Whether the step's checkbox is on.
    pub enabled: bool,
    /// Whether the step shows its dialog when played.
    pub with_dialog: bool,
    /// The step's parameters, if it has any.
    pub descriptor: Option<Descriptor>,
}

/// One action.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AtnAction {
    /// Display name (localisation keys already resolved, see [`zstring`]).
    pub name: String,
    /// Function key index (0 = none, 1–15 = F1–F15).
    pub fkey: u16,
    /// ⇧ is part of the function key shortcut.
    pub shift: bool,
    /// ⌘ is part of the function key shortcut.
    pub command: bool,
    /// Button-mode colour index.
    pub color: u16,
    /// Steps in order.
    pub steps: Vec<AtnStep>,
}

/// One action set (a folder in the Actions panel).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AtnSet {
    /// Display name (localisation keys already resolved).
    pub name: String,
    /// Actions in order.
    pub actions: Vec<AtnAction>,
}

/// A parsed actions file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AtnFile {
    /// File version (16).
    pub version: u32,
    /// Sets in order.
    pub sets: Vec<AtnSet>,
    /// Problems that did not stop the parse (where a truncated or unreadable file stopped).
    pub warnings: Vec<String>,
}

/// A Photoshop localisation string (`$$$/Presets/Actions/…=Default Actions`) as display text:
/// the text after `=`, or the last path segment when there is none. Plain strings pass through.
pub fn zstring(s: &str) -> String {
    let s = s.trim_end_matches('\0');
    let Some(rest) = s.strip_prefix("$$$/") else { return s.to_string() };
    match rest.split_once('=') {
        Some((_, text)) => text.to_string(),
        None => rest.rsplit('/').next().unwrap_or(rest).replace('_', " "),
    }
}

fn ustr(r: &mut Reader<'_>) -> Result<String> {
    Ok(zstring(&String::from_utf16_lossy(&read_unicode_units(r)?)))
}

fn ascii(r: &mut Reader<'_>) -> Result<String> {
    let n = r.u32()?;
    if n > 4096 {
        return Err(PsdError::LimitExceeded("action step name too long"));
    }
    Ok(String::from_utf8_lossy(r.bytes(n as usize)?).trim_end_matches('\0').to_string())
}

fn count(r: &mut Reader<'_>, what: &'static str) -> Result<u32> {
    let n = r.u32()?;
    if n > MAX_ITEMS {
        return Err(PsdError::LimitExceeded(what));
    }
    Ok(n)
}

fn read_step(r: &mut Reader<'_>) -> Result<AtnStep> {
    let _expanded = r.u8()?;
    let enabled = r.u8()? != 0;
    let with_dialog = r.u8()? != 0;
    let _dialog_options = r.u8()?;
    let kind = r.array::<4>()?;
    let event = match &kind {
        b"TEXT" => ascii(r)?,
        b"long" => String::from_utf8_lossy(&r.array::<4>()?).to_string(),
        other => return Err(PsdError::invalid(format!("unknown step id type {:?}", String::from_utf8_lossy(other)))),
    };
    let name = ascii(r)?;
    let descriptor = match r.i32()? {
        -1 => {
            let (d, used) = Descriptor::parse_prefix(r.peek_rest())?;
            r.skip(used)?;
            Some(d)
        }
        _ => None,
    };
    Ok(AtnStep { event, name, enabled, with_dialog, descriptor })
}

/// Reads one set; on a failure inside it, keeps what was read and returns the error beside it.
fn read_set(r: &mut Reader<'_>) -> (AtnSet, Option<String>) {
    let mut set = AtnSet::default();
    let header = (|| -> Result<u32> {
        set.name = ustr(r)?;
        let _expanded = r.u8()?;
        count(r, "too many actions in a set")
    })();
    let n = match header {
        Ok(n) => n,
        Err(e) => return (set, Some(format!("set header: {e}"))),
    };
    for ai in 0..n {
        let mut action = AtnAction::default();
        let head = (|| -> Result<u32> {
            action.fkey = r.u16()?;
            action.shift = r.u8()? != 0;
            action.command = r.u8()? != 0;
            action.color = r.u16()?;
            action.name = ustr(r)?;
            let _expanded = r.u8()?;
            count(r, "too many steps in an action")
        })();
        let steps = match head {
            Ok(s) => s,
            Err(e) => {
                let why = format!("action {} of \"{}\": {e}", ai + 1, set.name);
                return (set, Some(why));
            }
        };
        for si in 0..steps {
            match read_step(r) {
                Ok(s) => action.steps.push(s),
                Err(e) => {
                    let why = format!("\"{}\" step {}: {e}", action.name, si + 1);
                    set.actions.push(action);
                    return (set, Some(why));
                }
            }
        }
        set.actions.push(action);
    }
    (set, None)
}

/// Parses an `.atn` file (one set) or an `Actions Palette.psp` (u32 set count, then sets).
/// Never panics; a damaged file yields the sets read before the damage plus a warning, and only
/// a file with no readable set at all is an error.
pub fn parse(data: &[u8]) -> Result<AtnFile> {
    let mut r = Reader::new(data);
    let version = r.u32()?;
    if version != 16 {
        return Err(PsdError::invalid(format!("actions file version {version}, expected 16")));
    }
    let mut out = AtnFile { version, ..Default::default() };
    // A palette file has a small set count next; a single-set .atn has the set name's length.
    // Tell them apart by trying the palette layout first: its first set name must follow.
    let palette = {
        let mut probe = r.clone();
        matches!(probe.u32(), Ok(n) if n > 0 && n <= 1000) && {
            let mut p2 = probe.clone();
            matches!(p2.u32(), Ok(len) if len > 0 && len < 1024 && p2.remaining() >= len as usize * 2)
        }
    };
    let sets = if palette { count(&mut r, "too many action sets")? } else { 1 };
    for _ in 0..sets {
        let (set, err) = read_set(&mut r);
        let empty = set.name.is_empty() && set.actions.is_empty();
        if !empty {
            out.sets.push(set);
        }
        if let Some(e) = err {
            out.warnings.push(format!("stopped reading at {e}"));
            break;
        }
    }
    if out.sets.is_empty() {
        return Err(PsdError::invalid(out.warnings.first().cloned().unwrap_or_else(|| "no action sets".into())));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::Value;
    use crate::io::WriteExt;

    fn put_ustr(out: &mut Vec<u8>, s: &str) {
        let u: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        out.put_u32(u.len() as u32);
        for c in u {
            out.put_u16(c);
        }
    }

    fn put_ascii(out: &mut Vec<u8>, s: &str) {
        out.put_u32(s.len() as u32);
        out.extend_from_slice(s.as_bytes());
    }

    fn step(out: &mut Vec<u8>, event: &str, name: &str, d: Option<&Descriptor>) {
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(b"TEXT");
        put_ascii(out, event);
        put_ascii(out, name);
        match d {
            Some(d) => {
                out.put_i32(-1);
                out.extend_from_slice(&d.to_bytes());
            }
            None => out.put_i32(0),
        }
    }

    fn sample(palette: bool) -> Vec<u8> {
        let mut b = Vec::new();
        b.put_u32(16);
        if palette {
            b.put_u32(1);
        }
        put_ustr(&mut b, "$$$/Presets/Actions/Mine=My Set");
        b.push(1);
        b.put_u32(2);
        // Action 1: F2 + shift, two steps.
        b.put_u16(2);
        b.push(1);
        b.push(0);
        b.put_u16(3);
        put_ustr(&mut b, "Blur it");
        b.push(0);
        b.put_u32(2);
        let blur = Descriptor::new("GsnB").with("Rds ", Value::UnitFloat { unit: *b"#Pxl", value: 2.5 });
        step(&mut b, "gaussianBlur", "Gaussian Blur", Some(&blur));
        step(&mut b, "invert", "Invert", None);
        // Action 2: no steps.
        b.put_u16(0);
        b.push(0);
        b.push(0);
        b.put_u16(0);
        put_ustr(&mut b, "Empty");
        b.push(0);
        b.put_u32(0);
        b
    }

    #[test]
    fn reads_atn_and_palette_layouts() {
        for palette in [false, true] {
            let f = parse(&sample(palette)).unwrap();
            assert_eq!(f.sets.len(), 1);
            let s = &f.sets[0];
            assert_eq!(s.name, "My Set");
            assert_eq!(s.actions.len(), 2);
            let a = &s.actions[0];
            assert_eq!((a.name.as_str(), a.fkey, a.shift, a.color), ("Blur it", 2, true, 3));
            assert_eq!(a.steps.len(), 2);
            assert_eq!(a.steps[0].event, "gaussianBlur");
            assert!(matches!(a.steps[0].descriptor.as_ref().and_then(|d| d.get("Rds ")), Some(Value::UnitFloat { value, .. }) if (*value - 2.5).abs() < 1e-9));
            assert_eq!(a.steps[1].name, "Invert");
            assert!(a.steps[1].descriptor.is_none());
            assert!(f.warnings.is_empty());
        }
    }

    #[test]
    fn truncated_and_garbage_input_never_panics() {
        let full = sample(true);
        for cut in 0..full.len() {
            // Any prefix is an error or a partial parse with a warning, never a panic.
            if let Ok(f) = parse(&full[..cut]) {
                assert!(!f.sets.is_empty());
            }
        }
        let mut x: u32 = 0x1234_5678;
        for len in [0usize, 3, 8, 64, 512] {
            let mut g = vec![0, 0, 0, 16];
            for _ in 0..len {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                g.push(x as u8);
            }
            let _ = parse(&g);
        }
        assert!(parse(b"").is_err());
        assert!(parse(&[0, 0, 0, 15]).is_err());
    }

    #[test]
    fn zstrings_resolve() {
        assert_eq!(zstring("$$$/Presets/Actions/DefaultActions_atn/DefaultActions=Default Actions"), "Default Actions");
        assert_eq!(zstring("$$$/Presets/Actions/Default_Actions"), "Default Actions");
        assert_eq!(zstring("Plain\0"), "Plain");
    }
}
