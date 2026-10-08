//! Window › Swatches (the user's swatch groups) and saved guide layouts (View › New Guide Layout
//! presets).
//!
//! The Swatches panel always shows PhotoCraft's own built-in palette; these are the user's
//! groups on top of it: added by hand or imported from Photoshop (`.aco` / `Swatches.psp`).
//! Guide layout presets are named `view.newGuideLayout` parameter sets, imported from Photoshop
//! `.gds` files or saved by the user; `view.newGuideLayout {"preset": name}` applies one.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Group, Named, always, bad, edit_groups, group_index, req_str, str_param, unique_name};
use crate::commands::CommandSpec;
use crate::{Result, Session};

/// One swatch: a name and an 8-bit sRGB colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub rgb: [u8; 3],
}

impl Named for Swatch {
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, n: String) {
        self.name = n;
    }
}

/// A saved guide layout: `view.newGuideLayout` params.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GuideLayoutPreset {
    pub name: String,
    pub params: Value,
}

impl Named for GuideLayoutPreset {
    fn name(&self) -> &str {
        &self.name
    }
    fn set_name(&mut self, n: String) {
        self.name = n;
    }
}

/// Group for imported swatches outside any group.
pub const IMPORTED_GROUP: &str = "Imported Swatches";
/// Group for imported guide layouts.
pub const GUIDES_GROUP: &str = "Guide Layouts";

/// Photoshop's stock swatch names (its default RGB/CMYK/Grayscale/Pastel/Light/Pure/Dark/Darker/
/// brown rows), recognised by pattern so a migration can bring only the user's own colours.
pub fn is_stock_swatch(name: &str) -> bool {
    let n = name.trim();
    const PREFIXES: [&str; 11] = ["RGB ", "CMYK ", "Pastel ", "Light ", "Pure ", "Dark ", "Darker ", "Pale ", "Medium ", "Grayscale ", "Web "];
    matches!(n, "White" | "Black")
        || (n.ends_with("% Gray") && n.trim_end_matches("% Gray").chars().all(|c| c.is_ascii_digit()))
        || PREFIXES.iter().any(|p| n.starts_with(p))
}

/// Parse `#rrggbb`, `rrggbb` or `[r,g,b]`.
fn rgb_param(v: &Value) -> Option<[u8; 3]> {
    if let Some(a) = v.as_array() {
        let c = |i: usize| a.get(i).and_then(Value::as_f64).filter(|x| x.is_finite()).map(|x| x.clamp(0.0, 255.0).round() as u8);
        return Some([c(0)?, c(1)?, c(2)?]);
    }
    let s = v.as_str()?.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let h = |i: usize| s.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
    Some([h(0)?, h(2)?, h(4)?])
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let groups: Vec<Value> = s
        .presets
        .swatches
        .iter()
        .map(|g| json!({"name": g.name, "swatches": g.items.iter().map(|w| json!({"name": w.name, "color": hex(w.rgb)})).collect::<Vec<_>>()}))
        .collect();
    Ok(json!({"groups": groups}))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "swatches.edit";
    let action = req_str(p, "action", cmd)?;
    let r = if action == "add" {
        let rgb = p.get("color").and_then(rgb_param).ok_or_else(|| bad(cmd, "`color` must be \"#rrggbb\" or [r,g,b]"))?;
        let base = str_param(p, "name").unwrap_or("Swatch").to_string();
        let name = unique_name(&s.presets.swatches, &base);
        let gi = group_index(&mut s.presets.swatches, str_param(p, "group"));
        s.presets.swatches[gi].items.push(Swatch { name: name.clone(), rgb });
        json!({"name": name, "color": hex(rgb)})
    } else {
        edit_groups(&mut s.presets.swatches, action, p, cmd)?
    };
    s.presets_changed();
    Ok(r)
}

/// Swatches from an `.aco` / `Swatches.psp` file, grouped as the file groups them.
pub(crate) fn read_aco(bytes: &[u8], include_stock: bool) -> std::result::Result<(Vec<Group<Swatch>>, usize), String> {
    let colors = photocraft_psd::aco::parse(bytes).map_err(|e| format!("not a readable swatches file: {e}"))?;
    let mut groups: Vec<Group<Swatch>> = Vec::new();
    let mut skipped = 0;
    for c in colors {
        if !include_stock && is_stock_swatch(&c.name) {
            skipped += 1;
            continue;
        }
        let Some(rgb) = c.rgb8() else {
            skipped += 1;
            continue;
        };
        let name = if c.name.trim().is_empty() { hex(rgb) } else { c.name.trim().to_string() };
        let group = if c.group.is_empty() { IMPORTED_GROUP.to_string() } else { c.group.clone() };
        let gi = group_index(&mut groups, Some(&group));
        groups[gi].items.push(Swatch { name, rgb });
    }
    Ok((groups, skipped))
}

/// Merge imported swatch groups: a swatch whose name a group already has is replaced.
pub(crate) fn merge_swatches(s: &mut Session, groups: Vec<Group<Swatch>>) -> usize {
    let mut n = 0;
    for g in groups {
        let gi = group_index(&mut s.presets.swatches, Some(&g.name));
        for w in g.items {
            let items = &mut s.presets.swatches[gi].items;
            match items.iter_mut().find(|x| x.name == w.name) {
                Some(x) => *x = w,
                None => items.push(w),
            }
            n += 1;
        }
    }
    if n > 0 {
        s.presets_changed();
    }
    n
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "swatches.import";
    let (bytes, _) = crate::preset_import_cmds::file_bytes(p, cmd, ".aco file")?;
    let include_stock = p.get("includeStock").and_then(Value::as_bool).unwrap_or(true);
    let (groups, skipped) = read_aco(&bytes, include_stock).map_err(|e| bad(cmd, e))?;
    let names: Vec<String> = groups.iter().flat_map(|g| g.items.iter().map(|w| w.name.clone())).collect();
    merge_swatches(s, groups);
    Ok(json!({"imported": names, "count": names.len(), "skipped": skipped}))
}

/// `view.newGuideLayout` params from a Photoshop `.gds` guide layout preset.
pub(crate) fn read_gds(bytes: &[u8]) -> std::result::Result<Value, String> {
    use photocraft_psd::descriptor::Value as D;
    let vd = photocraft_psd::VersionedDescriptor::parse_prefix(bytes).map_err(|e| format!("not a readable guide layout: {e}"))?.0;
    let d = match vd.descriptor.get("guideLayout") {
        Some(D::Descriptor(d)) => d.clone(),
        _ => vd.descriptor,
    };
    let num = |k: &str| match d.get(k) {
        Some(D::Integer(i)) => Some(f64::from(*i)),
        Some(D::Double(x)) => Some(*x),
        Some(D::UnitFloat { value, .. }) => Some(*value),
        _ => None,
    };
    let mut p = serde_json::Map::new();
    for (k, to) in [("colCount", "columns"), ("rowCount", "rows")] {
        if let Some(n) = num(k).filter(|n| n.is_finite() && *n >= 0.0) {
            p.insert(to.into(), json!(n.min(1000.0) as u64));
        }
    }
    for (k, to) in [("colWidth", "width"), ("colGutter", "gutter"), ("rowHeight", "height"), ("rowGutter", "rowGutter")] {
        if let Some(n) = num(k).filter(|n| n.is_finite() && *n > 0.0) {
            p.insert(to.into(), json!(n));
        }
    }
    let margins: Vec<f64> =
        ["marginTop", "marginLeft", "marginBottom", "marginRight"].iter().map(|k| num(k).filter(|n| n.is_finite()).unwrap_or(0.0)).collect();
    if margins.iter().any(|m| *m != 0.0) {
        p.insert("margin".into(), json!(margins));
    }
    if let Some(D::Boolean(b)) = d.get("centerColumns").or_else(|| d.get("centerCols")) {
        p.insert("centerColumns".into(), json!(b));
    }
    if let (Some(r), Some(g), Some(b)) = (num("GdCR"), num("GdCG"), num("GdCB")) {
        let c = |v: f64| v.clamp(0.0, 255.0).round() as u8;
        p.insert("color".into(), json!(hex([c(r), c(g), c(b)])));
    }
    if !p.contains_key("columns") && !p.contains_key("rows") && !p.contains_key("margin") {
        return Err("the guide layout has no columns, rows or margins".into());
    }
    Ok(Value::Object(p))
}

pub(crate) fn add_guide_layout(s: &mut Session, name: &str, params: Value) {
    let gi = group_index(&mut s.presets.guide_layouts, Some(GUIDES_GROUP));
    let items = &mut s.presets.guide_layouts[gi].items;
    match items.iter_mut().find(|x| x.name == name) {
        Some(x) => x.params = params,
        None => items.push(GuideLayoutPreset { name: name.to_string(), params }),
    }
    s.presets_changed();
}

/// The saved params of guide layout `name`.
pub fn guide_layout(s: &Session, name: &str) -> Option<Value> {
    s.presets.guide_layouts.iter().flat_map(|g| g.items.iter()).find(|x| x.name.eq_ignore_ascii_case(name)).map(|x| x.params.clone())
}

fn guide_layouts(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "view.guideLayout.presets";
    match str_param(p, "action").unwrap_or("list") {
        "list" => {}
        "add" => {
            let name = req_str(p, "name", cmd)?.to_string();
            let params = p.get("params").filter(|v| v.is_object()).cloned().ok_or_else(|| bad(cmd, "`params` must be view.newGuideLayout params"))?;
            add_guide_layout(s, &name, params);
        }
        "import" => {
            let (bytes, stem) = crate::preset_import_cmds::file_bytes(p, cmd, ".gds file")?;
            let params = read_gds(&bytes).map_err(|e| bad(cmd, e))?;
            let name = str_param(p, "name").map(str::to_string).unwrap_or(if stem.is_empty() { "Guide Layout".into() } else { stem });
            add_guide_layout(s, &name, params.clone());
            return Ok(json!({"name": name, "params": params}));
        }
        other => {
            let r = edit_groups(&mut s.presets.guide_layouts, other, p, cmd)?;
            s.presets_changed();
            return Ok(r);
        }
    }
    let all: Vec<Value> =
        s.presets.guide_layouts.iter().flat_map(|g| g.items.iter().map(move |x| json!({"group": g.name, "name": x.name, "params": x.params}))).collect();
    Ok(json!({"layouts": all}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "swatches.list",
            label: "Swatches",
            menu: &[],
            shortcut: None,
            params: r##"{} → {groups:[{name, swatches:[{name, color:"#rrggbb"}]}]} (the user's groups; the built-in palette is not listed)"##,
            enabled: always,
            journal: false,
            run: list,
        },
        CommandSpec {
            id: "swatches.edit",
            label: "Edit Swatches",
            menu: &[],
            shortcut: None,
            params: r##"{"action":"add","color":"#rrggbb"|[r,g,b],"name":str?,"group":str?} or the group edits: {"action":"rename|delete|move|newGroup|renameGroup|deleteGroup",…}"##,
            enabled: always,
            journal: false,
            run: edit,
        },
        CommandSpec {
            id: "swatches.import",
            label: "Import Swatches…",
            menu: &[],
            shortcut: None,
            params: r##"{"path":".aco or Swatches.psp"?,"data":base64?,"includeStock":bool=true (false skips Photoshop's stock colours)} → {imported:[names], count, skipped}"##,
            enabled: always,
            journal: false,
            run: import,
        },
        CommandSpec {
            id: "view.guideLayout.presets",
            label: "Guide Layout Presets",
            menu: &[],
            shortcut: None,
            params: r##"{"action":"list"} → {layouts:[{group,name,params}]}; {"action":"add","name":str,"params":{view.newGuideLayout params}}; {"action":"import","path":".gds","name":str?}; or the group edits (rename|delete|…)"##,
            enabled: always,
            journal: false,
            run: guide_layouts,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_names_are_recognised() {
        for n in ["RGB Red", "50% Gray", "White", "Pastel Cyan Blue", "Darker Warm Brown", "Pale Cool Brown"] {
            assert!(is_stock_swatch(n), "{n}");
        }
        for n in ["bwald", "bwald2", "Brand Red", "Gray"] {
            assert!(!is_stock_swatch(n), "{n}");
        }
    }

    #[test]
    fn swatch_commands_round_trip_and_reject_bad_input() {
        let mut s = Session::new();
        s.execute("swatches.edit", json!({"action": "add", "color": "#3d4633", "name": "bwald", "group": "Mine"})).unwrap();
        s.execute("swatches.edit", json!({"action": "add", "color": [248, 242, 218], "name": "bwald2", "group": "Mine"})).unwrap();
        let l = s.execute("swatches.list", json!({})).unwrap();
        assert_eq!(l["groups"][0]["swatches"][1]["color"], "#f8f2da");
        let saved = s.prefs_to_json();
        let mut t = Session::new();
        t.load_prefs_json(&saved).unwrap();
        assert_eq!(t.presets.swatches, s.presets.swatches, "swatches persist with the preferences");
        for bad in [json!({}), json!({"action": "add"}), json!({"action": "add", "color": "#zzzzzz"}), json!({"action": "nope"}), json!({"action": 3})] {
            assert!(s.execute("swatches.edit", bad.clone()).is_err(), "{bad}");
        }
        assert!(s.execute("swatches.import", json!({"data": "AAAA"})).is_err());
        assert!(s.execute("swatches.import", json!({})).is_err());
    }

    #[test]
    fn gds_layouts_import_and_apply() {
        use photocraft_psd::descriptor::{Descriptor, Value as D};
        let inner = Descriptor::new("guideLayout")
            .with("colCount", D::Integer(3))
            .with("GdCR", D::Integer(255))
            .with("GdCG", D::Integer(74))
            .with("GdCB", D::Integer(74));
        let bytes = photocraft_psd::VersionedDescriptor::new(Descriptor::new("null").with("guideLayout", D::Descriptor(inner))).to_bytes();
        let p = read_gds(&bytes).unwrap();
        assert_eq!(p["columns"], 3);
        assert_eq!(p["color"], "#ff4a4a");
        for cut in 0..bytes.len() {
            let _ = read_gds(&bytes[..cut]);
        }
        let mut s = Session::new();
        add_guide_layout(&mut s, "3 vertical", p);
        s.execute("file.new", json!({"width": 300, "height": 100})).unwrap();
        let r = s.execute("view.newGuideLayout", json!({"preset": "3 vertical"})).unwrap();
        assert!(r["vertical"].as_u64().unwrap() >= 2, "{r}");
        assert!(s.execute("view.newGuideLayout", json!({"preset": "missing"})).is_err());
        assert!(s.execute("view.guideLayout.presets", json!({"action": "add", "name": "x"})).is_err());
        assert!(s.execute("view.guideLayout.presets", json!({"action": "import", "data": "AAAA"})).is_err());
    }
}
