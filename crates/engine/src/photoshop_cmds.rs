//! Moving from Photoshop: keyboard shortcuts (`.kys`), actions (`.atn`), and File › Migrate from
//! Photoshop…, which reads a Photoshop settings folder (shortcuts, actions, brushes, patterns,
//! swatches) plus its guide layout presets into PhotoCraft's own preferences and preset store.
//!
//! Everything here only *reads* Photoshop's files. Photoshop's stock presets (its default brush,
//! pattern and swatch sets) are skipped unless asked for: they are Adobe's, and PhotoCraft has its
//! own. The user's own content is imported into the user's preset store at run time.

pub mod atn_map;
pub mod kys;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::commands::CommandSpec;
use crate::prefs::normalize_shortcut;
use crate::{EngineError, Result, Session};

fn bad(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}

/// Photoshop's stock brush groups (Photoshop 2020+ defaults).
pub const STOCK_BRUSH_GROUPS: &[&str] = &["General Brushes", "Dry Media Brushes", "Wet Media Brushes", "Special Effects Brushes", "Legacy Brushes"];
/// Photoshop's stock swatch groups.
pub const STOCK_SWATCH_GROUPS: &[&str] = &["RGB", "CMYK", "Grayscale", "Pastel", "Light", "Pure", "Dark", "Darker", "Pale", "Web", "Legacy Swatches"];

/// Photoshop's stock patterns (the Trees, Grass and Water sets), by name.
pub fn is_stock_pattern(name: &str) -> bool {
    let n = photocraft_psd::atn::zstring(name);
    n.starts_with("Tree Tile") || n.starts_with("Grass") || n.starts_with("Water")
}

/// The keyboard shortcut changes a shortcut file makes, without applying them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyPlan {
    /// Command id → main shortcut (`""` = none), where it differs from PhotoCraft's default.
    pub set: BTreeMap<String, String>,
    /// Command ids of the mapped table that return to their default.
    pub reset: Vec<String>,
    /// Command id → further shortcuts.
    pub extra: BTreeMap<String, Vec<String>>,
    /// Tool name → key, where it differs from PhotoCraft's default.
    pub tool_keys: BTreeMap<String, String>,
    /// Commands the file gives a shortcut that PhotoCraft has no command for.
    pub unmapped: Vec<Value>,
    /// Tools the file gives a key that PhotoCraft doesn't have.
    pub unmapped_tools: Vec<Value>,
    /// The Quick Export format the file's menu names imply (`jpg`), when it names one.
    pub quick_export_format: Option<String>,
}

/// Work out what importing `f` changes (a complete Photoshop shortcut set: a mapped command it
/// leaves out has no shortcut).
pub fn plan_keys(f: &kys::KysFile) -> KeyPlan {
    let mut plan = KeyPlan::default();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let norm = |d: Option<&str>| d.and_then(normalize_shortcut).unwrap_or_default();
    let mut take = |plan: &mut KeyPlan, id: &'static str, def: Option<&str>, keys: &[String]| {
        seen.insert(id);
        let main = keys.first().cloned().unwrap_or_default();
        if main != norm(def) {
            plan.set.insert(id.to_string(), main.clone());
        } else {
            plan.reset.push(id.to_string());
        }
        let rest: Vec<String> = keys.iter().skip(1).filter(|k| **k != main).cloned().collect();
        if !rest.is_empty() {
            plan.extra.insert(id.to_string(), rest);
        }
    };
    for c in &f.commands {
        match kys::map_command(c) {
            Some((id, def)) => take(&mut plan, id, def, &c.shortcuts),
            None if !c.shortcuts.is_empty() => {
                plan.unmapped.push(json!({"name": photocraft_psd::atn::zstring(&c.name), "photoshopId": c.id, "shortcuts": c.shortcuts}))
            }
            None => {}
        }
        if c.id == Some(3444) {
            let n = c.name.to_ascii_lowercase();
            plan.quick_export_format = ["jpg", "png", "gif", "webp"].iter().find(|f| n.ends_with(*f)).map(|f| f.to_string());
        }
    }
    for t in &f.tools {
        if t.ty == 1 {
            match kys::TOOLS.iter().find(|x| x.0.eq_ignore_ascii_case(t.name.trim())) {
                Some((name, def)) => {
                    if t.key != *def {
                        plan.tool_keys.insert(name.to_string(), t.key.clone());
                    }
                }
                None if !t.key.is_empty() => plan.unmapped_tools.push(json!({"name": t.name, "key": t.key})),
                None => {}
            }
        } else if let Some((_, id, def)) = kys::TOOL_ACTIONS.iter().find(|x| x.0 == t.ty) {
            let keys: Vec<String> = if t.key.is_empty() { Vec::new() } else { vec![t.key.clone()] };
            take(&mut plan, id, *def, &keys);
        } else if !t.key.is_empty() {
            plan.unmapped.push(json!({"name": t.name, "shortcuts": [t.key]}));
        }
    }
    // A complete file (Photoshop always writes one) unbinds mapped commands it leaves out.
    if f.commands.len() >= 20 {
        let table = kys::STATIC.iter().map(|e| (e.1, e.2)).chain(kys::DYNAMIC.iter().map(|e| (e.1, e.2)));
        for (id, def) in table {
            if !seen.contains(id) && def.is_some() {
                plan.set.insert(id.to_string(), String::new());
            }
        }
    }
    plan.reset.retain(|id| !plan.set.contains_key(id));
    plan
}

/// Apply a [`KeyPlan`]: overrides, extra shortcuts and tool keys replace earlier imports for the
/// mapped commands; shortcuts taken from other commands are removed from them (as Photoshop
/// does). Returns the `edit.keyboardShortcuts` summary (conflicts…).
pub fn apply_keys(s: &mut Session, plan: &KeyPlan) -> Result<Value> {
    let set: Map<String, Value> = plan.set.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let mapped: Vec<String> = plan.set.keys().chain(plan.reset.iter()).cloned().collect();
    let r = crate::presets::call(s, "edit.keyboardShortcuts", json!({"reset": plan.reset, "set": set, "allowUnknown": true, "removeConflicts": true}))?;
    let extra = plan.extra.clone();
    let tool_keys = plan.tool_keys.clone();
    let fmt = plan.quick_export_format.clone();
    s.edit_prefs(|p| {
        for id in &mapped {
            p.extra_shortcuts.remove(id);
        }
        p.extra_shortcuts.extend(extra.clone());
        for (name, _) in kys::TOOLS {
            p.tool_keys.remove(*name);
        }
        p.tool_keys.extend(tool_keys);
        // Extra keys are taken from the registry commands that had them.
        let taken: BTreeSet<String> = extra.values().flatten().filter_map(|k| normalize_shortcut(k)).collect();
        let owners: Vec<String> = crate::command_specs()
            .iter()
            .filter(|c| !mapped.iter().any(|m| m == c.id))
            .filter(|c| p.shortcut(c.id, c.shortcut).and_then(normalize_shortcut).is_some_and(|k| taken.contains(&k)))
            .map(|c| c.id.to_string())
            .collect();
        for id in owners {
            p.shortcuts.insert(id, String::new());
        }
        if let Some(f) = &fmt {
            let _ = p.set("export.quickExportFormat", json!(f));
        }
    });
    Ok(r)
}

fn key_report(plan: &KeyPlan, applied: Option<&Value>) -> Value {
    json!({
        "changed": plan.set.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "unbound": plan.set.iter().filter(|(_, v)| v.is_empty()).map(|(k, _)| k).collect::<Vec<_>>(),
        "extra": plan.extra,
        "toolKeys": plan.tool_keys,
        "quickExportFormat": plan.quick_export_format,
        "unmapped": plan.unmapped,
        "unmappedTools": plan.unmapped_tools,
        "conflicts": applied.and_then(|a| a.get("conflicts")).cloned().unwrap_or(json!([])),
    })
}

fn import_kys(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "edit.keyboardShortcuts.importKys";
    let text = match p.get("text").and_then(Value::as_str) {
        Some(t) => t.to_string(),
        None => {
            let (bytes, _) = crate::preset_import_cmds::file_bytes(p, cmd, ".kys or Keyboard Shortcuts.psp file")?;
            String::from_utf8_lossy(&bytes).into_owned()
        }
    };
    let f = kys::parse(&text).map_err(|e| bad(cmd, e))?;
    let plan = plan_keys(&f);
    let applied = if p.get("dryRun").and_then(Value::as_bool).unwrap_or(false) { None } else { Some(apply_keys(s, &plan)?) };
    Ok(key_report(&plan, applied.as_ref()))
}

/// Add the actions of a parsed `.atn` (replacing actions of the same set and name). Returns the
/// per-action report.
fn add_actions(s: &mut Session, f: &photocraft_psd::atn::AtnFile) -> Vec<Value> {
    let mut report = Vec::new();
    for set in &f.sets {
        for a in &set.actions {
            let (steps, unsupported, names) = atn_map::map_action(&a.steps);
            report.push(json!({
                "set": set.name, "action": a.name, "steps": names.len(), "runnable": steps.len(), "unsupported": unsupported,
                "functionKey": (a.fkey > 0).then(|| format!("{}{}F{}", if a.command { "Cmd+" } else { "" }, if a.shift { "Shift+" } else { "" }, a.fkey)),
            }));
            let action = crate::actions_cmds::Action { name: a.name.clone(), steps, set: set.name.clone(), unsupported, source_steps: names };
            match s.actions.list.iter_mut().find(|x| x.name == a.name && x.set == set.name) {
                Some(x) => *x = action,
                None => s.actions.list.push(action),
            }
        }
    }
    s.actions.touch();
    report
}

fn import_atn(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "actions.importAtn";
    if s.actions.recording.is_some() {
        return Err(bad(cmd, "stop recording first"));
    }
    let (bytes, _) = crate::preset_import_cmds::file_bytes(p, cmd, ".atn or Actions Palette.psp file")?;
    let f = photocraft_psd::atn::parse(&bytes).map_err(|e| bad(cmd, format!("not a readable actions file: {e}")))?;
    let actions = add_actions(s, &f);
    Ok(json!({"sets": f.sets.len(), "actions": actions, "warnings": f.warnings}))
}

// ------------------------------------------------------------------ migration

/// Photoshop's settings folder for the newest installed version (`~/Library/Preferences/Adobe
/// Photoshop 2026 Settings` on a Mac), if any.
pub fn default_settings_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let prefs = home.join("Library/Preferences");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&prefs)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("Adobe Photoshop 20") && n.ends_with(" Settings")))
        .collect();
    dirs.sort();
    dirs.pop()
}

/// Every Photoshop guide layout preset (`.gds`) on this machine, newest Photoshop version first,
/// one per file name.
pub fn default_guide_files() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return Vec::new() };
    let mut roots: Vec<PathBuf> = Vec::new();
    for base in [home.join("Library/Application Support/Adobe"), home.join("Library/Preferences")] {
        if let Ok(rd) = std::fs::read_dir(&base) {
            roots.extend(
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("Adobe Photoshop"))),
            );
        }
    }
    // Newest year first; Beta after the releases.
    roots.sort_by(|a, b| b.cmp(a));
    let mut out: Vec<PathBuf> = Vec::new();
    for r in roots {
        for dir in [r.join("Presets/Guides"), r.clone()] {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            let mut files: Vec<PathBuf> =
                rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("gds"))).collect();
            files.sort();
            for f in files {
                if !out.iter().any(|o| o.file_name() == f.file_name()) {
                    out.push(f);
                }
            }
        }
    }
    out
}

fn read_capped(path: &Path) -> std::result::Result<Vec<u8>, String> {
    let len = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?.len();
    if len > crate::preset_import_cmds::MAX_FILE_BYTES {
        return Err(format!("{}: too large ({len} bytes)", path.display()));
    }
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

const KINDS: [&str; 6] = ["shortcuts", "actions", "brushes", "patterns", "swatches", "guides"];

/// Everything read from a Photoshop settings folder, before any of it touches the session
/// (read on a worker thread when the migration runs as a background job).
#[derive(Default)]
struct Gathered {
    dir: PathBuf,
    want: Vec<String>,
    stock: bool,
    dry: bool,
    keys: Option<KeyPlan>,
    actions: Option<photocraft_psd::atn::AtnFile>,
    brushes: Option<photocraft_io::abr_map::AbrImport>,
    patterns: Option<Vec<photocraft_doc::Pattern>>,
    swatches: Option<Vec<photocraft_psd::aco::AcoColor>>,
    guides: Vec<(String, Value, String)>,
    errors: Vec<String>,
}

fn gather(p: &Value, ctx: &crate::jobs::JobCtx) -> Result<Gathered> {
    let cmd = "file.migrateFromPhotoshop";
    let dir = match p.get("settingsDir") {
        Some(v) => PathBuf::from(v.as_str().filter(|d| !d.trim().is_empty()).ok_or_else(|| bad(cmd, "`settingsDir` must be a folder path"))?),
        // Looking for the folder is opt-in (`"auto": true`, the menu item's default), so a bare
        // call never reads a large Photoshop install by surprise.
        None if p.get("auto").and_then(Value::as_bool) == Some(true) => {
            default_settings_dir().ok_or_else(|| bad(cmd, "no Photoshop settings folder found; pass `settingsDir`"))?
        }
        None => return Err(bad(cmd, "pass `settingsDir` (a Photoshop settings folder) or `\"auto\": true` to find the newest one")),
    };
    if !dir.is_dir() {
        return Err(bad(cmd, format!("{} is not a folder", dir.display())));
    }
    let want: Vec<String> = match p.get("only") {
        None | Some(Value::Null) => KINDS.iter().map(|k| k.to_string()).collect(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(_) => return Err(bad(cmd, "`only` must be a list of kinds")),
    };
    if let Some(k) = want.iter().find(|k| !KINDS.contains(&k.as_str())) {
        return Err(bad(cmd, format!("unknown kind `{k}` ({})", KINDS.join("|"))));
    }
    let guide_files: Vec<PathBuf> = match p.get("guideFiles").and_then(Value::as_array) {
        Some(a) => a.iter().filter_map(Value::as_str).map(PathBuf::from).collect(),
        None => default_guide_files(),
    };
    let mut g = Gathered {
        dir,
        stock: p.get("includeStock").and_then(Value::as_bool).unwrap_or(false),
        dry: p.get("dryRun").and_then(Value::as_bool).unwrap_or(false),
        ..Default::default()
    };
    let w = |k: &str| want.iter().any(|o| o == k);
    let dir = g.dir.clone();
    if w("shortcuts") {
        ctx.progress(0.02, "Reading keyboard shortcuts");
        let path = ["Keyboard Shortcuts.psp", "Keyboard Shortcuts Primary.psp"].iter().map(|f| dir.join(f)).find(|f| f.is_file());
        match path.ok_or_else(|| "no Keyboard Shortcuts.psp".to_string()).and_then(|f| read_capped(&f)).and_then(|b| kys::parse(&String::from_utf8_lossy(&b))) {
            Ok(f) => g.keys = Some(plan_keys(&f)),
            Err(e) => g.errors.push(format!("shortcuts: {e}")),
        }
    }
    if w("actions") {
        ctx.check()?;
        ctx.progress(0.05, "Reading actions");
        match read_capped(&dir.join("Actions Palette.psp")).and_then(|b| photocraft_psd::atn::parse(&b).map_err(|e| e.to_string())) {
            Ok(f) => g.actions = Some(f),
            Err(e) => g.errors.push(format!("actions: {e}")),
        }
    }
    if w("brushes") {
        ctx.check()?;
        match read_capped(&dir.join("Brushes.psp"))
            .and_then(|b| ctx.stage(0.1, 0.8, "Reading brushes", |ctl| photocraft_io::abr_map::read_abr_grouped(&b, ctl)))
        {
            Ok(imp) => g.brushes = Some(imp),
            Err(_) if ctx.cancelled() => return Err(EngineError::Cancelled),
            Err(e) => g.errors.push(format!("brushes: {e}")),
        }
    }
    if w("patterns") {
        ctx.check()?;
        ctx.progress(0.8, "Reading patterns");
        match read_capped(&dir.join("Patterns.psp")).and_then(|b| photocraft_io::pattern_map::read_pat(&b)) {
            Ok(pats) => g.patterns = Some(pats),
            Err(e) => g.errors.push(format!("patterns: {e}")),
        }
    }
    if w("swatches") {
        ctx.progress(0.95, "Reading swatches");
        match read_capped(&dir.join("Swatches.psp")).and_then(|b| photocraft_psd::aco::parse(&b).map_err(|e| e.to_string())) {
            Ok(c) => g.swatches = Some(c),
            Err(e) => g.errors.push(format!("swatches: {e}")),
        }
    }
    if w("guides") {
        for f in guide_files {
            let name = f.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            match read_capped(&f).and_then(|b| crate::presets::swatches::read_gds(&b)) {
                Ok(params) => g.guides.push((name, params, f.display().to_string())),
                Err(e) => g.errors.push(format!("guides: {e}")),
            }
        }
    }
    g.want = want;
    ctx.progress(1.0, "Importing");
    Ok(g)
}

fn apply_gathered(s: &mut Session, g: Gathered) -> Result<Value> {
    let Gathered { dir, want, stock, dry, keys, actions, brushes, patterns, swatches, guides, mut errors } = g;
    let w = |k: &str| want.iter().any(|o| o == k);
    let mut report = Map::new();
    report.insert("settingsDir".into(), json!(dir.display().to_string()));
    report.insert("dryRun".into(), json!(dry));
    if let Some(plan) = keys {
        let applied = if dry { None } else { apply_keys(s, &plan).map_err(|e| errors.push(format!("shortcuts: {e}"))).ok() };
        report.insert("shortcuts".into(), key_report(&plan, applied.as_ref()));
    }
    if let Some(f) = actions {
        let list = if dry {
            f.sets
                .iter()
                .flat_map(|set| {
                    set.actions.iter().map(move |a| {
                        let (run, unsupported, names) = atn_map::map_action(&a.steps);
                        json!({"set": set.name, "action": a.name, "steps": names.len(), "runnable": run.len(), "unsupported": unsupported})
                    })
                })
                .collect()
        } else if s.actions.recording.is_some() {
            errors.push("actions: stop recording first".into());
            Vec::new()
        } else {
            add_actions(s, &f)
        };
        report.insert("actions".into(), json!({"sets": f.sets.len(), "count": list.len(), "actions": list, "warnings": f.warnings}));
    }
    if let Some(imp) = brushes {
        let (keep, skipped): (Vec<_>, Vec<_>) = imp.presets.into_iter().partition(|b| stock || !STOCK_BRUSH_GROUPS.contains(&b.group.as_str()));
        let mut groups: Vec<String> = Vec::new();
        for b in &keep {
            if !groups.contains(&b.group) {
                groups.push(b.group.clone());
            }
        }
        let mut skipped_groups: Vec<String> = Vec::new();
        for b in &skipped {
            if !skipped_groups.contains(&b.group) {
                skipped_groups.push(b.group.clone());
            }
        }
        let count = keep.len();
        if !dry {
            // Re-migrating replaces the groups instead of doubling them.
            s.tools.presets.retain(|x| x.builtin || !groups.contains(&x.group));
            let mut taken: BTreeSet<String> = s.tools.presets.iter().map(|x| x.name.to_lowercase()).collect();
            for mut b in keep {
                let base = b.name.clone();
                let mut n = 2;
                while taken.contains(&b.name.to_lowercase()) {
                    b.name = format!("{base} {n}");
                    n += 1;
                }
                taken.insert(b.name.to_lowercase());
                s.tools.presets.push(b);
            }
            s.brush_presets_changed();
        }
        report.insert(
            "brushes".into(),
            json!({"count": count, "groups": groups, "skippedStock": skipped.len(), "skippedGroups": skipped_groups, "warnings": imp.warnings}),
        );
    }
    if let Some(pats) = patterns {
        let (keep, skipped): (Vec<_>, Vec<_>) = pats.into_iter().partition(|p| stock || !is_stock_pattern(&p.name));
        let names: Vec<String> = keep.iter().map(|p| p.display_name().to_string()).collect();
        if !dry && !keep.is_empty() {
            let ids: Vec<String> = keep.iter().map(|p| p.id.clone()).collect();
            for pat in keep {
                match s.patterns.items.iter_mut().find(|q| q.id == pat.id) {
                    Some(q) => *q = pat,
                    None => s.patterns.items.push(pat),
                }
            }
            let groups = &mut s.presets.pattern_groups;
            for g in groups.iter_mut() {
                g.items.retain(|id| !ids.contains(id));
            }
            let gi = crate::presets::group_index(groups, Some("Photoshop Patterns"));
            groups[gi].items.extend(ids);
            s.presets_changed();
        }
        report.insert("patterns".into(), json!({"count": names.len(), "names": names, "skippedStock": skipped.len()}));
    }
    if let Some(colors) = swatches {
        let total = colors.len();
        let mut groups: Vec<crate::presets::Group<crate::presets::swatches::Swatch>> = Vec::new();
        for c in colors {
            let stocky = STOCK_SWATCH_GROUPS.contains(&c.group.as_str()) || crate::presets::swatches::is_stock_swatch(&c.name);
            let Some(rgb) = c.rgb8().filter(|_| stock || !stocky) else { continue };
            let group = if c.group.is_empty() { crate::presets::swatches::IMPORTED_GROUP.to_string() } else { c.group.clone() };
            let gi = crate::presets::group_index(&mut groups, Some(&group));
            groups[gi].items.push(crate::presets::swatches::Swatch { name: c.name.trim().to_string(), rgb });
        }
        let list: Vec<Value> = groups
            .iter()
            .flat_map(|g| {
                g.items.iter().map(move |w| json!({"group": g.name, "name": w.name, "color": format!("#{:02x}{:02x}{:02x}", w.rgb[0], w.rgb[1], w.rgb[2])}))
            })
            .collect();
        if !dry {
            crate::presets::swatches::merge_swatches(s, groups);
        }
        report.insert("swatches".into(), json!({"count": list.len(), "swatches": list, "skippedStock": total - list.len()}));
    }
    if w("guides") {
        let mut done = Vec::new();
        for (name, params, from) in guides {
            if !dry {
                crate::presets::swatches::add_guide_layout(s, &name, params.clone());
            }
            done.push(json!({"name": name, "params": params, "from": from}));
        }
        report.insert("guides".into(), json!({"count": done.len(), "layouts": done}));
    }
    // Styles and gradients in a Photoshop settings folder are Photoshop's stock sets unless the
    // user made some; their formats are not imported here either way.
    report.insert(
        "notImported".into(),
        json!([
            "Styles.psp (layer styles: .asl import not implemented)",
            "Gradients.psp (stock; use Import Gradients for your own .grd files)",
            "ToolPresets.psp",
            "CustomShapes.psp"
        ]),
    );
    report.insert("errors".into(), json!(errors));
    Ok(Value::Object(report))
}

fn migrate(s: &mut Session, p: &Value) -> Result<Value> {
    let params = p.clone();
    crate::jobs::run(s, "Migrate from Photoshop", false, move |ctx| gather(&params, ctx), apply_gathered)
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "edit.keyboardShortcuts.importKys",
            label: "Import Photoshop Shortcuts…",
            menu: &[],
            shortcut: None,
            params: r##"{"path":".kys or Keyboard Shortcuts.psp"?,"data":base64?,"text":xml?,"dryRun":bool=false} → {changed:[[id,key]], unbound:[id], extra:{id:[keys]}, toolKeys, unmapped, unmappedTools, conflicts}. The file is a complete set: mapped commands it leaves out lose their shortcut."##,
            enabled: always,
            journal: false,
            run: import_kys,
        },
        CommandSpec {
            id: "actions.importAtn",
            label: "Load Actions…",
            menu: &[],
            shortcut: None,
            params: r##"{"path":".atn or Actions Palette.psp"?,"data":base64?} → {sets, actions:[{set, action, steps, runnable, unsupported:[step names]}], warnings}. Actions keep their set; one of the same set and name is replaced."##,
            enabled: always,
            journal: false,
            run: import_atn,
        },
        CommandSpec {
            id: "file.migrateFromPhotoshop",
            label: "Migrate from Photoshop…",
            menu: &["File"],
            shortcut: None,
            params: r##"{"settingsDir":path | "auto":true (the newest ~/Library/Preferences/Adobe Photoshop 20xx Settings),"only":["shortcuts","actions","brushes","patterns","swatches","guides"]?,"includeStock":bool=false (also bring Photoshop's stock brushes, patterns and swatches),"guideFiles":[".gds"]? (default: every Photoshop Presets/Guides folder),"dryRun":bool=false} → report per kind, plus errors. Reads only; writes PhotoCraft's preferences and preset store."##,
            enabled: always,
            journal: false,
            run: migrate,
        },
    ]
}
