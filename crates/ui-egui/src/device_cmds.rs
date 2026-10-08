//! Commands for driving PhotoCraft from devices that send keystrokes (Logitech MX Creative
//! Console / Loupedeck, Stream Deck, programmable mice): pick a tool directly (no cycling through
//! its group), step the brush or layer opacity and the layer blend mode, plus default keys for
//! menu commands Photoshop leaves without one but device profiles use. See
//! `tools/devices/README.md` for the Photoshop action → PhotoCraft command → key table.
//!
//! Default keys use F13–F19 and ⌃⌥⇧ + letter, which nothing else binds.

use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::state::Tool;

/// `tools.select.<tool>` ids.
pub const TOOL_IDS: [(Tool, &str); 49] = [
    (Tool::Move, "tools.select.move"),
    (Tool::RectMarquee, "tools.select.rectMarquee"),
    (Tool::EllipseMarquee, "tools.select.ellipseMarquee"),
    (Tool::Lasso, "tools.select.lasso"),
    (Tool::PolygonLasso, "tools.select.polygonLasso"),
    (Tool::MagneticLasso, "tools.select.magneticLasso"),
    (Tool::MagicWand, "tools.select.magicWand"),
    (Tool::Crop, "tools.select.crop"),
    (Tool::Eyedropper, "tools.select.eyedropper"),
    (Tool::Ruler, "tools.select.ruler"),
    (Tool::Note, "tools.select.note"),
    (Tool::Count, "tools.select.count"),
    (Tool::Brush, "tools.select.brush"),
    (Tool::Pencil, "tools.select.pencil"),
    (Tool::MixerBrush, "tools.select.mixerBrush"),
    (Tool::Eraser, "tools.select.eraser"),
    (Tool::BackgroundEraser, "tools.select.backgroundEraser"),
    (Tool::MagicEraser, "tools.select.magicEraser"),
    (Tool::Gradient, "tools.select.gradient"),
    (Tool::PaintBucket, "tools.select.paintBucket"),
    (Tool::Type, "tools.select.type"),
    (Tool::VerticalType, "tools.select.verticalType"),
    (Tool::Hand, "tools.select.hand"),
    (Tool::Zoom, "tools.select.zoom"),
    (Tool::SpotHealing, "tools.select.spotHealing"),
    (Tool::Healing, "tools.select.healing"),
    (Tool::Patch, "tools.select.patch"),
    (Tool::ContentAwareMove, "tools.select.contentAwareMove"),
    (Tool::CloneStamp, "tools.select.cloneStamp"),
    (Tool::HistoryBrush, "tools.select.historyBrush"),
    (Tool::Blur, "tools.select.blur"),
    (Tool::Sharpen, "tools.select.sharpen"),
    (Tool::Smudge, "tools.select.smudge"),
    (Tool::Dodge, "tools.select.dodge"),
    (Tool::Burn, "tools.select.burn"),
    (Tool::Sponge, "tools.select.sponge"),
    (Tool::QuickSelection, "tools.select.quickSelection"),
    (Tool::ObjectSelection, "tools.select.objectSelection"),
    (Tool::Pen, "tools.select.pen"),
    (Tool::PathSelection, "tools.select.pathSelection"),
    (Tool::DirectSelection, "tools.select.directSelection"),
    (Tool::Rectangle, "tools.select.rectangle"),
    (Tool::EllipseShape, "tools.select.ellipseShape"),
    (Tool::Triangle, "tools.select.triangle"),
    (Tool::Polygon, "tools.select.polygon"),
    (Tool::Line, "tools.select.line"),
    (Tool::CustomShape, "tools.select.customShape"),
    (Tool::Slice, "tools.select.slice"),
    (Tool::SliceSelect, "tools.select.sliceSelect"),
];

/// Device commands handled here: (id, label, default shortcut). Tool picks are added from
/// [`TOOL_IDS`] (see [`commands`]).
pub const COMMANDS: &[(&str, &str, Option<&str>)] = &[
    ("tools.opacityDown", "Decrease Tool Opacity", Some("Shift+F15")),
    ("tools.opacityUp", "Increase Tool Opacity", Some("Shift+F16")),
    ("tools.layerOpacityDown", "Decrease Layer Opacity", Some("Shift+F13")),
    ("tools.layerOpacityUp", "Increase Layer Opacity", Some("Shift+F14")),
    ("tools.blendModePrevious", "Previous Blend Mode", Some("Shift+-")),
    ("tools.blendModeNext", "Next Blend Mode", Some("Shift+=")),
];

/// Default keys for tool picks device profiles use (the rest have none).
pub const TOOL_KEYS: &[(&str, &str)] = &[
    ("tools.select.spotHealing", "Ctrl+Alt+Shift+J"),
    ("tools.select.patch", "Ctrl+Alt+Shift+P"),
    ("tools.select.quickSelection", "Ctrl+Alt+Shift+Q"),
    ("tools.select.magicWand", "Ctrl+Alt+Shift+W"),
    ("tools.select.lasso", "Ctrl+Alt+Shift+L"),
    ("tools.select.magneticLasso", "Ctrl+Alt+Shift+G"),
    ("tools.select.crop", "Ctrl+Alt+Shift+C"),
    ("tools.select.move", "Ctrl+Alt+Shift+V"),
];

/// Default keys for existing menu commands that have none in Photoshop but that device
/// profiles drive: (id, shortcut).
pub const MENU_KEYS: &[(&str, &str)] = &[
    ("layer.newAdjustmentLayer.levels", "F13"),
    ("layer.layerMask.revealSelection", "F14"),
    ("image.adjustments.exposure", "F15"),
    ("edit.contentAwareFill", "F16"),
    ("layer.newAdjustmentLayer.hueSaturation", "F17"),
];

/// Every device command: (id, label, default).
pub fn commands() -> Vec<(&'static str, String, Option<&'static str>)> {
    let mut v: Vec<(&'static str, String, Option<&'static str>)> =
        TOOL_IDS.iter().map(|(t, id)| (*id, format!("Select {}", t.label()), TOOL_KEYS.iter().find(|k| k.0 == *id).map(|k| k.1))).collect();
    v.extend(COMMANDS.iter().map(|(id, l, d)| (*id, l.to_string(), *d)));
    v
}

/// The default shortcut of a device command or device-bound menu command.
pub fn default_shortcut(id: &str) -> Option<&'static str> {
    TOOL_KEYS.iter().chain(MENU_KEYS.iter()).find(|k| k.0 == id).map(|k| k.1).or_else(|| COMMANDS.iter().find(|c| c.0 == id).and_then(|c| c.2))
}

pub fn handles(id: &str) -> bool {
    TOOL_IDS.iter().any(|t| t.1 == id) || COMMANDS.iter().any(|c| c.0 == id)
}

fn step_layer(app: &mut PhotocraftApp, delta: f32) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document open")?;
    let layer = st.active_layer.and_then(|id| st.doc.layer(id)).ok_or("no active layer")?;
    if crate::doc_props_ui::is_background(&st.doc, layer) {
        return Err("the Background layer has no opacity".into());
    }
    let v = ((layer.opacity + delta) * 100.0).round().clamp(0.0, 100.0) / 100.0;
    let id = layer.id.0;
    app.run("layer.setProps", json!({"layer": id, "opacity": v}))?;
    Ok(json!({"opacity": v}))
}

fn step_blend(app: &mut PhotocraftApp, dir: i32) -> Result<Value, String> {
    use photocraft_color::BlendMode;
    let st = app.session.active().ok_or("no document open")?;
    let layer = st.active_layer.and_then(|id| st.doc.layer(id)).ok_or("no active layer")?;
    if crate::doc_props_ui::is_background(&st.doc, layer) {
        return Err("the Background layer has no blend mode".into());
    }
    let modes = BlendMode::LAYER_MODES;
    let at = modes.iter().position(|m| *m == layer.blend).unwrap_or(0) as i32;
    let n = modes.len() as i32;
    let next = modes.get((at + dir).rem_euclid(n.max(1)) as usize).copied().unwrap_or(BlendMode::Normal);
    let id = layer.id.0;
    app.run("layer.setProps", json!({"layer": id, "blend": next.label()}))?;
    Ok(json!({"blend": next.label()}))
}

fn step_tool_opacity(app: &mut PhotocraftApp, delta: f32) -> Result<Value, String> {
    if app.ui.tool.is_brushlike() {
        let v = ((app.session.tools.brush.opacity + delta) * 100.0).round().clamp(1.0, 100.0) / 100.0;
        app.run("tools.setBrush", json!({"brush": {"opacity": v}}))?;
        return Ok(json!({"opacity": v}));
    }
    if matches!(app.ui.tool, Tool::Gradient | Tool::PaintBucket) {
        let v = (app.ui.tool_options.fill_opacity + delta * 100.0).round().clamp(1.0, 100.0);
        app.ui.tool_options.fill_opacity = v;
        return Ok(json!({"opacity": v / 100.0}));
    }
    // Tools without an opacity of their own step the layer's, as the number keys do.
    step_layer(app, delta)
}

/// Run a device command; `None` when `id` isn't one.
pub fn menu(app: &mut PhotocraftApp, id: &str, _params: &Value) -> Option<Result<Value, String>> {
    if let Some((tool, _)) = TOOL_IDS.iter().find(|t| t.1 == id) {
        app.ui.tool = *tool;
        return Some(Ok(json!({"tool": format!("{tool:?}")})));
    }
    Some(match id {
        "tools.opacityDown" => step_tool_opacity(app, -0.1),
        "tools.opacityUp" => step_tool_opacity(app, 0.1),
        "tools.layerOpacityDown" => step_layer(app, -0.1),
        "tools.layerOpacityUp" => step_layer(app, 0.1),
        "tools.blendModePrevious" => step_blend(app, -1),
        "tools.blendModeNext" => step_blend(app, 1),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_pick_command_and_device_keys_are_unique() {
        for t in Tool::ALL {
            assert!(TOOL_IDS.iter().any(|x| x.0 == t), "{t:?}");
        }
        let mut keys: Vec<String> = TOOL_KEYS
            .iter()
            .map(|k| k.1)
            .chain(MENU_KEYS.iter().map(|k| k.1))
            .chain(COMMANDS.iter().filter_map(|c| c.2))
            .filter_map(photocraft_engine::prefs::normalize_shortcut)
            .collect();
        let n = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), n, "a device key is bound twice");
        for (id, _) in MENU_KEYS {
            assert!(crate::menus::is_live(id), "{id} is not a live command");
        }
    }

    #[test]
    fn device_commands_run_and_fail_gracefully() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        let ctx = egui::Context::default();
        assert!(crate::menus::invoke(&mut app, &ctx, "tools.layerOpacityDown", json!({})).is_err(), "no document");
        assert!(crate::menus::invoke(&mut app, &ctx, "tools.blendModeNext", json!({})).is_err());
        crate::menus::invoke(&mut app, &ctx, "tools.select.patch", json!({})).unwrap();
        assert_eq!(app.ui.tool, Tool::Patch);
        app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
        app.run("layer.new.layer", json!({})).unwrap();
        app.ui.tool = Tool::Move;
        crate::menus::invoke(&mut app, &ctx, "tools.layerOpacityDown", json!({})).unwrap();
        crate::menus::invoke(&mut app, &ctx, "tools.opacityDown", json!({})).unwrap();
        let st = app.session.active().unwrap();
        let l = st.doc.layer(st.active_layer.unwrap()).unwrap();
        assert!((l.opacity - 0.8).abs() < 1e-4, "{}", l.opacity);
        let r = crate::menus::invoke(&mut app, &ctx, "tools.blendModeNext", json!({})).unwrap();
        assert_ne!(r["blend"], "Normal");
        crate::menus::invoke(&mut app, &ctx, "tools.blendModePrevious", json!({})).unwrap();
        let st = app.session.active().unwrap();
        assert_eq!(st.doc.layer(st.active_layer.unwrap()).unwrap().blend, photocraft_color::BlendMode::Normal);
        app.ui.tool = Tool::Brush;
        crate::menus::invoke(&mut app, &ctx, "tools.opacityDown", json!({})).unwrap();
        assert!(app.session.tools.brush.opacity < 1.0);
    }

    /// The Photoshop import tables name live commands with PhotoCraft's real defaults, and its
    /// tool list is PhotoCraft's toolbar.
    #[test]
    fn photoshop_tables_match_the_shell() {
        use photocraft_engine::photoshop_cmds::kys;
        use photocraft_engine::prefs::normalize_shortcut;
        let n = |s: Option<&str>| s.and_then(normalize_shortcut);
        for (ps, id, def) in kys::STATIC
            .iter()
            .map(|e| (e.0.to_string(), e.1, e.2))
            .chain(kys::DYNAMIC.iter().map(|e| (e.0.to_string(), e.1, e.2)))
            .chain(kys::TOOL_ACTIONS.iter().map(|e| (e.0.to_string(), e.1, e.2)))
        {
            assert!(crate::menus::is_live(id), "Photoshop {ps} → {id} is not live");
            assert_eq!(n(crate::shortcuts::default_shortcut(id).as_deref()), n(def), "{id}: the table's default differs from the shell's");
            if let Some(d) = def {
                assert!(crate::shortcuts::parse(&normalize_shortcut(d).unwrap()).is_some(), "{id}: {d} does not parse");
            }
        }
        let tools: Vec<(String, String)> =
            Tool::ALL.iter().map(|t| (t.label().to_string(), if t.key() == '\0' { String::new() } else { t.key().to_string() })).collect();
        let table: Vec<(String, String)> = kys::TOOLS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        for t in &tools {
            assert!(table.contains(t), "{t:?} missing from kys::TOOLS");
        }
        assert_eq!(tools.len(), table.len());
    }

    /// Bryan's Photoshop changes reach the key dispatcher.
    #[test]
    fn imported_photoshop_keys_dispatch() {
        use egui::{Key, Modifiers};
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        let kys = "<photoshop-keyboard-shortcuts version=\"4\">\
            <command kind=\"static\" id=\"3444\" name=\"Quick Export as JPG\"><shortcut>Cmd+E</shortcut></command>\
            <command kind=\"static\" id=\"1157\" name=\"Reveal All\"><shortcut>Cmd+X</shortcut></command>\
            <command kind=\"static\" id=\"103\" name=\"Cut\"><shortcut>F2</shortcut></command>\
            <command kind=\"static\" id=\"104\" name=\"Copy\"><shortcut>Cmd+C</shortcut><shortcut>F3</shortcut></command>\
            <tool name=\"Sponge Tool\" type=\"1\" key=\"1\">K</tool>\
            </photoshop-keyboard-shortcuts>";
        app.run("edit.keyboardShortcuts.importKys", json!({"text": kys})).unwrap();
        let b = crate::shortcut_dispatch::bindings(&app);
        let owner = |key: Key, m: Modifiers| b.iter().find(|(_, sc)| crate::shortcuts::key_matches(sc, key, m)).map(|(id, _)| id.clone());
        assert_eq!(owner(Key::E, Modifiers::COMMAND).as_deref(), Some("file.export.quickExportAsPng"));
        assert_eq!(owner(Key::X, Modifiers::COMMAND).as_deref(), Some("image.revealAll"));
        assert_eq!(owner(Key::F2, Modifiers::NONE).as_deref(), Some("edit.cut"));
        assert_eq!(owner(Key::F3, Modifiers::NONE).as_deref(), Some("edit.copy"));
        assert_eq!(owner(Key::F16, Modifiers::NONE).as_deref(), Some("edit.contentAwareFill"));
        assert_eq!(crate::shortcuts::tool_key(&app, Tool::Sponge), Some(Key::K));
        assert_eq!(crate::shortcuts::tool_key(&app, Tool::Brush), Some(Key::B));
    }
}
