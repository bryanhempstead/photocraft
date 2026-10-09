//! Photoshop's Color Settings, read from the Mac PhotoCraft runs on (Bryan's fork: Adobe data
//! installed on the machine may be read at runtime; nothing Adobe is committed).
//!
//! Photoshop keeps the active Edit › Color Settings in `~/Library/Preferences/Adobe Photoshop
//! 20xx Settings/Color Settingscsf`, an ICC-style tag table (`AsCs`): the working RGB / CMYK /
//! Gray profiles are embedded whole (`wRGB`, `wCMY`, `wGry`; `prof` + 4 bytes, then the ICC
//! data), the rest are `ui32` values: rendering intent `cInt` (ICC numbering), black point
//! compensation `kpc `, dither `dith`, the policies `pRGB` / `pCMY` / `pGry` (`pres`, `conv`,
//! `off `), the profile warnings `mAsk` / `pAsk` / `misA`, "Blend Text Colors Using Gamma"
//! `txge` / `txgv` (gamma × 100) and "Blend RGB Colors Using Gamma" `bge ` / `bgv `.
//!
//! The working profiles are reachable as the profile specs `photoshop-rgb`, `photoshop-cmyk` and
//! `photoshop-gray` (see [`crate::color_cmds::resolve_profile`]), so a saved setting keeps
//! following Photoshop's file. Without a Photoshop installation everything here is `None` and
//! PhotoCraft's built-in defaults stay in force.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use photocraft_cms::Intent;
use serde_json::{Value, json};

use crate::color_cmds::{ColorSettings, Policy};
use crate::commands::CommandSpec;
use crate::{EngineError, Result, Session};

/// Photoshop's Color Settings as stored in a `.csf` file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PsColorSettings {
    /// The settings' name (`Color Settingscsf` for the active set, or the preset's name).
    pub name: String,
    /// "Adobe Photoshop 6.0"-style description of the preset Photoshop started from.
    pub description: String,
    pub rgb: Option<Arc<Vec<u8>>>,
    pub cmyk: Option<Arc<Vec<u8>>>,
    pub gray: Option<Arc<Vec<u8>>>,
    pub intent: Option<Intent>,
    pub bpc: Option<bool>,
    pub dither: Option<bool>,
    pub policy_rgb: Option<Policy>,
    pub policy_cmyk: Option<Policy>,
    pub policy_gray: Option<Policy>,
    pub ask_on_mismatch: Option<bool>,
    pub ask_on_paste: Option<bool>,
    pub ask_on_missing: Option<bool>,
    /// Blend Text Colors Using Gamma (1.0 when off).
    pub text_gamma: Option<f32>,
    /// Blend RGB Colors Using Gamma (`None` when off; PhotoCraft blends in the document's
    /// encoding, which is Photoshop's behaviour with the option off).
    pub rgb_blend_gamma: Option<f32>,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at.checked_add(4)?).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes)
}

/// Parses a Color Settings (`.csf`) file. Unknown or malformed tags are skipped.
pub fn parse_csf(bytes: &[u8]) -> std::result::Result<PsColorSettings, String> {
    if bytes.get(36..40) != Some(b"AsCs".as_slice()) {
        return Err("not a Photoshop Color Settings file (no AsCs signature)".into());
    }
    let count = be32(bytes, 128).ok_or("truncated tag table")? as usize;
    let mut out = PsColorSettings::default();
    let mut text_gamma_on = None;
    let mut text_gamma = None;
    let mut rgb_gamma_on = None;
    let mut rgb_gamma = None;
    for i in 0..count.min(512) {
        let e = 132 + 12 * i;
        let (Some(sig), Some(off), Some(len)) = (bytes.get(e..e + 4), be32(bytes, e + 4), be32(bytes, e + 8)) else { break };
        let Some(v) = bytes.get(off as usize..(off as usize).saturating_add(len as usize)) else { continue };
        let ui = || (v.get(0..4) == Some(b"ui32".as_slice())).then(|| be32(v, 8)).flatten();
        let text = || {
            let n = be32(v, 8)? as usize;
            let s = v.get(16..16usize.checked_add(n)?)?;
            Some(String::from_utf8_lossy(s).trim_end_matches('\0').to_string())
        };
        let icc = || (v.get(0..4) == Some(b"prof".as_slice())).then(|| v.get(8..)).flatten().filter(|b| b.len() > 128).map(|b| Arc::new(b.to_vec()));
        let policy = |x: u32| match &x.to_be_bytes() {
            b"pres" => Some(Policy::Preserve),
            b"conv" => Some(Policy::Convert),
            b"off " => Some(Policy::Off),
            _ => None,
        };
        match sig {
            b"name" => out.name = text().unwrap_or_default(),
            b"wNam" => out.description = text().unwrap_or_default(),
            b"wRGB" => out.rgb = icc(),
            b"wCMY" => out.cmyk = icc(),
            b"wGry" => out.gray = icc(),
            b"cInt" => out.intent = ui().and_then(Intent::from_u32),
            b"kpc " => out.bpc = ui().map(|x| x != 0),
            b"dith" => out.dither = ui().map(|x| x != 0),
            b"pRGB" => out.policy_rgb = ui().and_then(policy),
            b"pCMY" => out.policy_cmyk = ui().and_then(policy),
            b"pGry" => out.policy_gray = ui().and_then(policy),
            b"mAsk" => out.ask_on_mismatch = ui().map(|x| x != 0),
            b"pAsk" => out.ask_on_paste = ui().map(|x| x != 0),
            b"misA" => out.ask_on_missing = ui().map(|x| x != 0),
            b"txge" => text_gamma_on = ui().map(|x| x != 0),
            b"txgv" => text_gamma = ui().map(|x| x as f32 / 100.0),
            b"bge " => rgb_gamma_on = ui().map(|x| x != 0),
            b"bgv " => rgb_gamma = ui().map(|x| x as f32 / 100.0),
            _ => {}
        }
    }
    out.text_gamma = match (text_gamma_on, text_gamma) {
        (Some(false), _) => Some(1.0),
        (Some(true), Some(g)) if (1.0..=2.2).contains(&g) => Some(g),
        _ => None,
    };
    out.rgb_blend_gamma = match (rgb_gamma_on, rgb_gamma) {
        (Some(true), Some(g)) if g > 0.0 => Some(g),
        _ => None,
    };
    Ok(out)
}

/// The active Color Settings file of the newest Photoshop on this Mac.
pub fn default_csf_path() -> Option<PathBuf> {
    let p = crate::photoshop_cmds::default_settings_dir()?.join("Color Settingscsf");
    p.is_file().then_some(p)
}

/// Photoshop's active Color Settings on this machine (read once per process).
pub fn installed() -> Option<&'static PsColorSettings> {
    static CELL: OnceLock<Option<PsColorSettings>> = OnceLock::new();
    CELL.get_or_init(|| {
        if std::env::var_os("PHOTOCRAFT_NO_PHOTOSHOP_COLOR").is_some() {
            return None;
        }
        let bytes = std::fs::read(default_csf_path()?).ok()?;
        parse_csf(&bytes).ok()
    })
    .as_ref()
}

/// Working profile bytes for the `photoshop-rgb` / `photoshop-cmyk` / `photoshop-gray` specs.
pub fn working_bytes(spec: &str) -> Option<Arc<Vec<u8>>> {
    let ps = installed()?;
    match spec {
        "photoshop-rgb" => ps.rgb.clone(),
        "photoshop-cmyk" => ps.cmyk.clone(),
        "photoshop-gray" => ps.gray.clone(),
        _ => None,
    }
}

/// `base` with every value Photoshop's settings define. Working spaces become the
/// `photoshop-*` specs (when `live`) so they keep following Photoshop's file; the monitor
/// profile is left alone (Photoshop takes it from macOS, as PhotoCraft's `auto` does).
pub fn apply(ps: &PsColorSettings, base: &ColorSettings, live: bool) -> ColorSettings {
    let mut c = base.clone();
    if live {
        if ps.rgb.is_some() {
            c.working_rgb = "photoshop-rgb".into();
        }
        if ps.cmyk.is_some() {
            c.working_cmyk = "photoshop-cmyk".into();
        }
        if ps.gray.is_some() {
            c.working_gray = "photoshop-gray".into();
        }
    }
    if let Some(i) = ps.intent {
        c.intent = i.id().into();
    }
    if let Some(v) = ps.bpc {
        c.bpc = v;
    }
    if let Some(v) = ps.dither {
        c.dither = v;
    }
    for (src, dst) in [(ps.policy_rgb, &mut c.policy_rgb), (ps.policy_cmyk, &mut c.policy_cmyk), (ps.policy_gray, &mut c.policy_gray)] {
        if let Some(p) = src {
            *dst = p;
        }
    }
    for (src, dst) in [(ps.ask_on_mismatch, &mut c.ask_on_mismatch), (ps.ask_on_paste, &mut c.ask_on_paste), (ps.ask_on_missing, &mut c.ask_on_missing)] {
        if let Some(v) = src {
            *dst = v;
        }
    }
    if let Some(g) = ps.text_gamma {
        c.blend_text_gamma = g;
    }
    c
}

fn describe(ps: &PsColorSettings) -> Value {
    let desc = |b: &Option<Arc<Vec<u8>>>| b.as_ref().and_then(|b| photocraft_cms::Profile::parse(b).ok()).map(|p| p.description);
    json!({
        "name": ps.name, "description": ps.description,
        "workingRgb": desc(&ps.rgb), "workingCmyk": desc(&ps.cmyk), "workingGray": desc(&ps.gray),
        "intent": ps.intent.map(Intent::id), "bpc": ps.bpc, "dither": ps.dither,
        "policyRgb": ps.policy_rgb.map(Policy::id), "policyCmyk": ps.policy_cmyk.map(Policy::id), "policyGray": ps.policy_gray.map(Policy::id),
        "askOnMismatch": ps.ask_on_mismatch, "askOnPaste": ps.ask_on_paste, "askOnMissing": ps.ask_on_missing,
        "blendTextGamma": ps.text_gamma, "blendRgbGamma": ps.rgb_blend_gamma,
    })
}

impl Session {
    /// Startup hook (desktop app): when Color Settings were never changed in PhotoCraft (they
    /// equal the built-in defaults) and Photoshop's Color Settings are installed, use Photoshop's,
    /// so documents look and convert the way they do in Photoshop. Returns what was adopted.
    pub fn adopt_photoshop_color_settings(&mut self) -> Option<Value> {
        let ps = installed()?;
        if self.color.settings != ColorSettings::default() {
            return None;
        }
        let next = apply(ps, &self.color.settings, true);
        crate::color_cmds::validate_settings(&next).ok()?;
        if next == self.color.settings {
            return None;
        }
        photocraft_compose::psblend::set_text_gamma(next.blend_text_gamma);
        self.color.settings = next;
        self.prefs.edit(|_| ());
        Some(describe(ps))
    }
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "edit.colorSettings.importPhotoshop";
    let bad = |msg: String| EngineError::BadParams { cmd: cmd.into(), msg };
    let (ps, live) = match p.get("path").and_then(Value::as_str) {
        Some(path) => {
            let bytes = std::fs::read(Path::new(path)).map_err(|e| bad(format!("cannot read `{path}`: {e}")))?;
            // A preset file brings its intent, BPC, dither, policies and warnings; its embedded
            // working profiles are not stored, so the working spaces stay as they are.
            (parse_csf(&bytes).map_err(bad)?, false)
        }
        None => (installed().cloned().ok_or_else(|| bad("no Photoshop Color Settings found on this machine (pass `path`)".into()))?, true),
    };
    let next = apply(&ps, &s.color.settings, live);
    crate::color_cmds::validate_settings(&next).map_err(bad)?;
    let dry = p.get("dryRun").and_then(Value::as_bool).unwrap_or(false);
    let changed = next != s.color.settings;
    if changed && !dry {
        photocraft_compose::psblend::set_text_gamma(next.blend_text_gamma);
        s.color.settings = next.clone();
        s.prefs.edit(|_| ());
    }
    Ok(json!({"photoshop": describe(&ps), "settings": next, "changed": changed, "dryRun": dry,
              "notes": if ps.rgb_blend_gamma.is_some() { vec!["Blend RGB Colors Using Gamma is on in Photoshop; PhotoCraft blends without it"] } else { vec![] }}))
}

fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "edit.colorSettings.importPhotoshop",
        label: "Use Photoshop's Color Settings",
        menu: &[],
        shortcut: None,
        params: r##"{"path":".csf"? (default: the newest Photoshop's active Color Settings on this Mac),"dryRun":bool=false} → {photoshop:{workingRgb, workingCmyk, workingGray, intent, bpc, policies…}, settings, changed}. Working spaces follow Photoshop's file live (photoshop-rgb / photoshop-cmyk / photoshop-gray)."##,
        enabled: always,
        journal: false,
        run: import,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal `.csf`: header with the AsCs signature, a tag table and values.
    fn csf(tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0u8; 128];
        out[36..40].copy_from_slice(b"AsCs");
        out.extend((tags.len() as u32).to_be_bytes());
        let mut data_off = 132 + 12 * tags.len();
        let mut data: Vec<u8> = Vec::new();
        for (sig, v) in tags {
            out.extend_from_slice(*sig);
            out.extend((data_off as u32).to_be_bytes());
            out.extend((v.len() as u32).to_be_bytes());
            data.extend(v);
            data_off += v.len();
        }
        out.extend(data);
        out
    }
    fn ui(v: u32) -> Vec<u8> {
        let mut b = b"ui32\0\0\0\0".to_vec();
        b.extend(v.to_be_bytes());
        b
    }

    #[test]
    fn parses_north_america_general_purpose() {
        let srgb = photocraft_cms::Builtin::Srgb.profile().to_bytes();
        let mut prof = b"prof\0\0\0\0".to_vec();
        prof.extend(srgb.iter());
        let b = csf(&[
            (b"wRGB", prof),
            (b"cInt", ui(1)),
            (b"kpc ", ui(1)),
            (b"dith", ui(1)),
            (b"pRGB", ui(u32::from_be_bytes(*b"pres"))),
            (b"pCMY", ui(u32::from_be_bytes(*b"conv"))),
            (b"mAsk", ui(0)),
            (b"txge", ui(1)),
            (b"txgv", ui(145)),
            (b"bge ", ui(0)),
        ]);
        let ps = parse_csf(&b).unwrap();
        assert_eq!(ps.rgb.as_deref().map(Vec::len), Some(srgb.len()));
        assert_eq!(ps.intent, Some(Intent::RelativeColorimetric));
        assert_eq!((ps.bpc, ps.dither), (Some(true), Some(true)));
        assert_eq!((ps.policy_rgb, ps.policy_cmyk), (Some(Policy::Preserve), Some(Policy::Convert)));
        assert_eq!(ps.ask_on_mismatch, Some(false));
        assert_eq!(ps.text_gamma, Some(1.45));
        assert_eq!(ps.rgb_blend_gamma, None);
        let c = apply(&ps, &ColorSettings::default(), false);
        assert!(!c.ask_on_mismatch);
        assert_eq!(c.policy_cmyk, Policy::Convert);
        assert_eq!(c.working_cmyk, ColorSettings::default().working_cmyk);
    }

    #[test]
    fn malformed_files_are_errors_not_panics() {
        assert!(parse_csf(&[]).is_err());
        assert!(parse_csf(&[0u8; 200]).is_err());
        let mut b = csf(&[(b"cInt", ui(1))]);
        b.truncate(150);
        assert!(parse_csf(&b).is_ok());
        // A tag pointing past the end, and a huge count.
        let mut b = csf(&[(b"wRGB", b"prof\0\0\0\0xx".to_vec())]);
        b[128..132].copy_from_slice(&u32::MAX.to_be_bytes());
        let ps = parse_csf(&b).unwrap();
        assert!(ps.rgb.is_none());
    }

    #[test]
    fn import_command_rejects_bad_paths() {
        let mut s = Session::new();
        assert!(s.execute("edit.colorSettings.importPhotoshop", json!({"path": "/nonexistent/x.csf"})).is_err());
    }

    /// On a Mac with Photoshop: its active settings import and every working space resolves.
    #[test]
    fn installed_settings_import_when_present() {
        let Some(ps) = installed() else { return };
        let mut s = Session::new();
        let r = s.execute("edit.colorSettings.importPhotoshop", json!({})).unwrap();
        eprintln!("{}", r["photoshop"]);
        for (mode, has) in [
            (photocraft_color::ColorMode::Rgb, ps.rgb.is_some()),
            (photocraft_color::ColorMode::Cmyk, ps.cmyk.is_some()),
            (photocraft_color::ColorMode::Grayscale, ps.gray.is_some()),
        ] {
            if has {
                let p = s.color.working(mode);
                assert!(!p.description.contains("Photocraft"), "{mode:?}: {}", p.description);
            }
        }
    }
}
