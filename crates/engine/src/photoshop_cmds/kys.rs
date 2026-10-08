//! Photoshop keyboard shortcut files (`.kys`, and `Keyboard Shortcuts.psp` in Photoshop's
//! preferences folder: the same XML).
//!
//! ```xml
//! <photoshop-keyboard-shortcuts version="4" …>
//!   <command kind="static" id="104" name="Copy"><shortcut>Cmd+C</shortcut><shortcut>F3</shortcut></command>
//!   <command kind="dynamic" name="Liquify"><shortcut>Shift+Cmd+X</shortcut></command>
//!   <tool name="Brush Tool" type="1" key="1886286946">B</tool>
//!   <tool name="Default Foreground/Background Colors" type="2">D</tool>
//! </photoshop-keyboard-shortcuts>
//! ```
//!
//! A shortcut file lists every command that has a shortcut; one it leaves out has none. The
//! mapping tables below translate Photoshop's command ids (stable across languages) and tool names
//! into PhotoCraft command ids and tool names, each with PhotoCraft's own default key so an
//! import only records what differs.

/// One `<command>`.
#[derive(Clone, Debug, PartialEq)]
pub struct KysCommand {
    /// `static` (menu item, has an id) or `dynamic` (plug-in filter, matched by name).
    pub kind: String,
    pub id: Option<u32>,
    pub name: String,
    /// Shortcuts in PhotoCraft notation (`Cmd+Alt+Shift+K`), the first being the main one.
    pub shortcuts: Vec<String>,
}

/// One `<tool>`: a toolbar tool (type 1) or a tool-related action (types 2+).
#[derive(Clone, Debug, PartialEq)]
pub struct KysTool {
    pub name: String,
    pub ty: u32,
    /// The key in PhotoCraft notation, empty for none.
    pub key: String,
}

/// A parsed shortcut file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KysFile {
    pub commands: Vec<KysCommand>,
    pub tools: Vec<KysTool>,
}

/// Most entries read from one file.
const MAX_ENTRIES: usize = 20_000;

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let pat = format!(" {name}=\"");
    let at = tag.find(&pat)? + pat.len();
    let rest = tag.get(at..)?;
    let end = rest.find('"')?;
    Some(unescape(rest.get(..end)?))
}

/// Photoshop key text (`Opt+Shift+Cmd+K`, `Control+Cmd+F`, `Cmd++`, `{`) in PhotoCraft notation
/// (`Cmd+Alt+Shift+K`, `Cmd+Ctrl+F`, `Cmd+=`, `Shift+[`); `None` for an empty or unreadable one.
pub fn ps_key(s: &str) -> Option<String> {
    let s = unescape(s.trim());
    if s.is_empty() {
        return None;
    }
    let (mods, key) = match s.strip_suffix("++") {
        Some(m) => (m.to_string(), "+".to_string()),
        None if s == "+" => (String::new(), "+".to_string()),
        None => match s.rsplit_once('+') {
            Some((m, k)) => (m.to_string(), k.to_string()),
            None => (String::new(), s.clone()),
        },
    };
    let mut parts: Vec<String> = Vec::new();
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        parts.push(
            match m.to_ascii_lowercase().as_str() {
                "cmd" | "command" => "Cmd",
                "opt" | "option" | "alt" => "Alt",
                "control" | "ctrl" => "Ctrl",
                "shift" => "Shift",
                _ => return None,
            }
            .to_string(),
        );
    }
    // Shifted punctuation is the unshifted key with ⇧, as PhotoCraft spells it; `+` is `=`.
    let (shift, key) = match key.as_str() {
        "{" => (true, "["),
        "}" => (true, "]"),
        "<" => (true, ","),
        ">" => (true, "."),
        "+" => (false, "="),
        k => (false, k),
    };
    if shift {
        parts.push("Shift".into());
    }
    parts.push(key.to_string());
    crate::prefs::normalize_shortcut(&parts.join("+"))
}

/// Parses a shortcut file. Never panics; unreadable entries are skipped, and only text that is
/// not a Photoshop shortcut file at all is an error.
pub fn parse(text: &str) -> Result<KysFile, String> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.contains("<photoshop-keyboard-shortcuts") {
        return Err("not a Photoshop keyboard shortcuts file".into());
    }
    let mut out = KysFile::default();
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        if out.commands.len() + out.tools.len() >= MAX_ENTRIES {
            break;
        }
        rest = rest.get(at..).unwrap_or_default();
        let Some(close) = rest.find('>') else { break };
        let tag = rest.get(..=close).unwrap_or_default();
        let after = rest.get(close + 1..).unwrap_or_default();
        let self_closing = tag.ends_with("/>");
        if tag.starts_with("<command ") {
            let (body, next) = if self_closing {
                ("", after)
            } else {
                match after.find("</command>") {
                    Some(e) => (after.get(..e).unwrap_or_default(), after.get(e + 10..).unwrap_or_default()),
                    None => (after, ""),
                }
            };
            let mut shortcuts = Vec::new();
            let mut b = body;
            while let Some(s) = b.find("<shortcut>") {
                let inner = b.get(s + 10..).unwrap_or_default();
                let e = inner.find("</shortcut>").unwrap_or(inner.len());
                if let Some(k) = ps_key(inner.get(..e).unwrap_or_default())
                    && !shortcuts.contains(&k)
                {
                    shortcuts.push(k);
                }
                b = inner.get(e..).unwrap_or_default();
            }
            out.commands.push(KysCommand {
                kind: attr(tag, "kind").unwrap_or_default(),
                id: attr(tag, "id").and_then(|i| i.parse().ok()),
                name: attr(tag, "name").unwrap_or_default(),
                shortcuts,
            });
            rest = next;
        } else if tag.starts_with("<tool ") {
            let (key, next) = if self_closing {
                (String::new(), after)
            } else {
                match after.find("</tool>") {
                    Some(e) => (after.get(..e).unwrap_or_default().to_string(), after.get(e + 7..).unwrap_or_default()),
                    None => (String::new(), ""),
                }
            };
            out.tools.push(KysTool {
                name: attr(tag, "name").unwrap_or_default(),
                ty: attr(tag, "type").and_then(|t| t.parse().ok()).unwrap_or(0),
                key: ps_key(&key).unwrap_or_default(),
            });
            rest = next;
        } else {
            rest = after;
        }
    }
    Ok(out)
}

/// Photoshop menu command id → (PhotoCraft command id, PhotoCraft's default key). The ids are
/// Photoshop's (language independent); PhotoCraft's defaults are checked against the shell's
/// binding table by a ui-egui test.
pub const STATIC: &[(u32, &str, Option<&str>)] = &[
    (10, "file.new", Some("Cmd+N")),
    (20, "file.open", Some("Cmd+O")),
    (30, "file.save", Some("Cmd+S")),
    (31, "file.close", Some("Cmd+W")),
    (32, "file.saveAs", Some("Cmd+Shift+S")),
    (33, "file.saveACopy", Some("Cmd+Alt+S")),
    (34, "file.revert", Some("F12")),
    (36, "file.exit", Some("Cmd+Q")),
    (37, "file.closeAll", Some("Cmd+Alt+W")),
    (46, "file.closeOthers", Some("Cmd+Alt+P")),
    (101, "edit.undo", Some("Cmd+Z")),
    (103, "edit.cut", Some("Cmd+X")),
    (104, "edit.copy", Some("Cmd+C")),
    (105, "edit.paste", Some("Cmd+V")),
    (132, "edit.redo", Some("Cmd+Shift+Z")),
    (133, "edit.toggleLastState", Some("Cmd+Alt+Z")),
    (177, "file.printOneCopy", Some("Cmd+Alt+Shift+P")),
    (1002, "view.rulers", Some("Cmd+R")),
    (1004, "view.zoomIn", Some("Cmd+=")),
    (1005, "view.zoomOut", Some("Cmd+-")),
    (1016, "select.deselect", Some("Cmd+D")),
    (1017, "select.all", Some("Cmd+A")),
    (1018, "select.inverse", Some("Cmd+Shift+I")),
    (1019, "filter.lastFilter", Some("Cmd+Alt+F")),
    (1025, "window.toggle.brushSettings", Some("F5")),
    (1030, "image.imageSize", Some("Cmd+Alt+I")),
    (1031, "image.canvasSize", Some("Cmd+Alt+C")),
    (1036, "select.modify.feather", Some("Shift+F6")),
    (1040, "edit.pasteSpecial.pasteInto", Some("Cmd+Alt+Shift+V")),
    (1042, "edit.fill", Some("Shift+F5")),
    (1046, "window.toggle.color", Some("F6")),
    (1055, "window.panel.info", Some("F8")),
    (1098, "window.toggle.layers", Some("F7")),
    (1099, "layer.new.layer", Some("Cmd+Shift+N")),
    (1105, "view.proofColors", Some("Cmd+Y")),
    (1106, "view.gamutWarning", Some("Cmd+Shift+Y")),
    (1107, "edit.copyMerged", Some("Cmd+Shift+C")),
    (1114, "layer.hideLayers", Some("Cmd+,")),
    (1115, "select.selectAndMask", Some("Cmd+Alt+R")),
    (1137, "file.fileInfo", Some("Cmd+Alt+Shift+I")),
    (1139, "layer.mergeVisible", Some("Cmd+Shift+E")),
    (1154, "edit.fade", Some("Cmd+Shift+F")),
    (1157, "image.revealAll", None),
    (1166, "layer.mergeLayers", Some("Cmd+E")),
    (1170, "window.panel.actions", Some("Alt+F9")),
    (1190, "view.actualPixels", Some("Cmd+1")),
    (1192, "view.fitOnScreen", Some("Cmd+0")),
    (1297, "edit.pasteSpecial.pasteInPlace", Some("Cmd+Shift+V")),
    (1695, "file.export.saveForWebLegacy", Some("Cmd+Alt+Shift+S")),
    (1701, "image.adjustments.invert", Some("Cmd+I")),
    (1801, "image.adjustments.levels", Some("Cmd+L")),
    (1802, "image.adjustments.curves", Some("Cmd+M")),
    (1804, "image.adjustments.colorBalance", Some("Cmd+B")),
    (1805, "image.adjustments.hueSaturation", Some("Cmd+U")),
    (1808, "image.autoTone", Some("Cmd+Shift+L")),
    (1809, "image.adjustments.desaturate", Some("Cmd+Shift+U")),
    (1810, "image.autoContrast", Some("Cmd+Alt+Shift+L")),
    (1817, "image.autoColor", Some("Cmd+Shift+B")),
    (1824, "image.adjustments.blackWhite", Some("Cmd+Alt+Shift+B")),
    (1943, "select.reselect", Some("Cmd+Shift+D")),
    (2101, "file.print", Some("Cmd+P")),
    (2207, "edit.freeTransform", Some("Cmd+T")),
    (2217, "edit.transform.again", Some("Cmd+Shift+T")),
    (2220, "edit.contentAwareScale", Some("Cmd+Alt+Shift+C")),
    (2311, "edit.preferences.general", None),
    (2344, "edit.colorSettings", Some("Cmd+Shift+K")),
    (2711, "layer.arrange.bringToFront", Some("Cmd+Shift+]")),
    (2712, "layer.arrange.bringForward", Some("Cmd+]")),
    (2713, "layer.arrange.sendBackward", Some("Cmd+[")),
    (2714, "layer.arrange.sendToBack", Some("Cmd+Shift+[")),
    (2860, "image.analysis.recordMeasurements", None),
    (2940, "view.lockGuides", Some("Cmd+Alt+;")),
    (2957, "layer.lockLayers", None),
    (2958, "layer.groupLayers", Some("Cmd+G")),
    (2959, "layer.ungroupLayers", Some("Cmd+Shift+G")),
    (2962, "select.allLayers", Some("Cmd+Alt+A")),
    (2970, "layer.new.layerViaCopy", Some("Cmd+J")),
    (2971, "layer.new.layerViaCut", Some("Cmd+Shift+J")),
    (2972, "layer.createClippingMask", Some("Cmd+Alt+G")),
    (2982, "select.findLayers", Some("Cmd+Alt+Shift+F")),
    (3443, "file.export.exportAs", Some("Cmd+Alt+Shift+W")),
    (3444, "file.export.quickExportAsPng", None),
    (3446, "layer.quickExportAsPng", Some("Cmd+Shift+'")),
    (3447, "layer.exportAs", Some("Cmd+Alt+Shift+'")),
    (3500, "view.extras", Some("Cmd+H")),
    (3502, "view.show.targetPath", Some("Cmd+Shift+H")),
    (3503, "view.show.guides", Some("Cmd+;")),
    (3504, "view.show.grid", Some("Cmd+'")),
    (3520, "view.snap", Some("Cmd+Shift+;")),
    (5957, "edit.search", Some("Cmd+K")),
    (5980, "edit.keyboardShortcuts", Some("Cmd+Alt+Shift+K")),
    (5982, "edit.menus", Some("Cmd+Alt+Shift+M")),
];

/// Photoshop plug-in filters (`kind="dynamic"`, matched by name) → PhotoCraft command and default.
pub const DYNAMIC: &[(&str, &str, Option<&str>)] = &[
    ("Vanishing Point", "filter.vanishingPoint", Some("Cmd+Alt+V")),
    ("Liquify", "filter.liquify", Some("Cmd+Shift+X")),
    ("Lens Correction", "filter.lensCorrection", Some("Cmd+Shift+R")),
    ("Camera Raw Filter", "filter.cameraRaw", Some("Cmd+Shift+A")),
    ("Wide Angle Correction", "filter.adaptiveWideAngle", Some("Cmd+Alt+Shift+A")),
];

/// Tool actions (`<tool type="2…">`) → PhotoCraft command and default.
pub const TOOL_ACTIONS: &[(u32, &str, Option<&str>)] = &[
    (2, "tools.defaultColors", Some("D")),
    (3, "tools.swapColors", Some("X")),
    (4, "select.editInQuickMaskMode", Some("Q")),
    (5, "view.screenMode.cycle", Some("F")),
    (7, "tools.decreaseBrushSize", Some("[")),
    (8, "tools.increaseBrushSize", Some("]")),
    (9, "tools.decreaseBrushHardness", Some("Shift+[")),
    (10, "tools.increaseBrushHardness", Some("Shift+]")),
];

/// PhotoCraft's toolbar tools by Photoshop name, with their default single key (`""` = none).
/// The ui-egui `Tool` list is checked against it by a test.
pub const TOOLS: &[(&str, &str)] = &[
    ("Move Tool", "V"),
    ("Rectangular Marquee Tool", "M"),
    ("Elliptical Marquee Tool", "M"),
    ("Brush Tool", "B"),
    ("Pencil Tool", "B"),
    ("Mixer Brush Tool", "B"),
    ("Eraser Tool", "E"),
    ("Background Eraser Tool", "E"),
    ("Magic Eraser Tool", "E"),
    ("Eyedropper Tool", "I"),
    ("Ruler Tool", "I"),
    ("Note Tool", "I"),
    ("Count Tool", "I"),
    ("Lasso Tool", "L"),
    ("Polygonal Lasso Tool", "L"),
    ("Magnetic Lasso Tool", "L"),
    ("Magic Wand Tool", "W"),
    ("Crop Tool", "C"),
    ("Slice Tool", "C"),
    ("Slice Select Tool", "C"),
    ("Gradient Tool", "G"),
    ("Paint Bucket Tool", "G"),
    ("Horizontal Type Tool", "T"),
    ("Vertical Type Tool", "T"),
    ("Hand Tool", "H"),
    ("Zoom Tool", "Z"),
    ("Spot Healing Brush Tool", "J"),
    ("Healing Brush Tool", "J"),
    ("Patch Tool", "J"),
    ("Content-Aware Move Tool", "J"),
    ("Clone Stamp Tool", "S"),
    ("History Brush Tool", "Y"),
    ("Blur Tool", ""),
    ("Sharpen Tool", ""),
    ("Smudge Tool", ""),
    ("Dodge Tool", "O"),
    ("Burn Tool", "O"),
    ("Sponge Tool", "O"),
    ("Quick Selection Tool", "W"),
    ("Object Selection Tool", "W"),
    ("Pen Tool", "P"),
    ("Path Selection Tool", "A"),
    ("Direct Selection Tool", "A"),
    ("Rectangle Tool", "U"),
    ("Ellipse Tool", "U"),
    ("Triangle Tool", "U"),
    ("Polygon Tool", "U"),
    ("Line Tool", "U"),
    ("Custom Shape Tool", "U"),
];

/// The PhotoCraft command for a parsed `<command>` and its default.
pub fn map_command(c: &KysCommand) -> Option<(&'static str, Option<&'static str>)> {
    match c.id {
        Some(id) => STATIC.iter().find(|e| e.0 == id).map(|e| (e.1, e.2)),
        None => DYNAMIC.iter().find(|e| e.0.eq_ignore_ascii_case(c.name.trim())).map(|e| (e.1, e.2)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}<?xml version=\"1.0\"?>\n<photoshop-keyboard-shortcuts version=\"4\">\n\
        <command kind=\"dynamic\" name=\"Liquify\"><shortcut>Shift+Cmd+X</shortcut></command>\n\
        <command kind=\"static\" id=\"104\" name=\"Copy\"><shortcut>Cmd+C</shortcut><shortcut>F3</shortcut></command>\n\
        <command kind=\"static\" id=\"1004\" name=\"Zoom In\"><shortcut>Cmd++</shortcut><shortcut>Cmd+=</shortcut></command>\n\
        <command kind=\"static\" id=\"1824\" name=\"Black &amp; White...\"><shortcut>Opt+Shift+Cmd+B</shortcut></command>\n\
        <command kind=\"static\" id=\"5985\" name=\"Quick Compare\"><shortcut></shortcut></command>\n\
        <tool name=\"Brush Tool\" type=\"1\" key=\"1886286946\">B</tool>\n\
        <tool name=\"Blur Tool\" type=\"1\" key=\"1\"></tool>\n\
        <tool name=\"First Brush\" type=\"13\">&lt;</tool>\n\
        </photoshop-keyboard-shortcuts>";

    #[test]
    fn parses_commands_and_tools() {
        let f = parse(SAMPLE).unwrap();
        assert_eq!(f.commands.len(), 5);
        assert_eq!(f.commands[0].shortcuts, vec!["Cmd+Shift+X"]);
        assert_eq!(f.commands[1].shortcuts, vec!["Cmd+C", "F3"]);
        assert_eq!(f.commands[2].shortcuts, vec!["Cmd+="], "Cmd++ and Cmd+= are one key");
        assert_eq!(f.commands[3].name, "Black & White...");
        assert_eq!(f.commands[3].shortcuts, vec!["Cmd+Alt+Shift+B"]);
        assert!(f.commands[4].shortcuts.is_empty());
        assert_eq!(f.tools[0].key, "B");
        assert_eq!(f.tools[1].key, "");
        assert_eq!(f.tools[2].key, "Shift+,");
        assert_eq!(map_command(&f.commands[0]), Some(("filter.liquify", Some("Cmd+Shift+X"))));
        assert_eq!(map_command(&f.commands[1]).map(|m| m.0), Some("edit.copy"));
    }

    #[test]
    fn key_spellings() {
        assert_eq!(ps_key("Control+Cmd+F").as_deref(), Some("Cmd+Ctrl+F"));
        assert_eq!(ps_key("Opt+F9").as_deref(), Some("Alt+F9"));
        assert_eq!(ps_key("Shift+Cmd+'").as_deref(), Some("Cmd+Shift+'"));
        assert_eq!(ps_key("{").as_deref(), Some("Shift+["));
        assert_eq!(ps_key("Bogus+K"), None);
        assert_eq!(ps_key(""), None);
    }

    #[test]
    fn truncated_and_garbage_never_panic() {
        for cut in 0..SAMPLE.len() {
            if let Some(t) = SAMPLE.get(..cut) {
                let _ = parse(t);
            }
        }
        assert!(parse("hello").is_err());
        for junk in [
            "<photoshop-keyboard-shortcuts",
            "<photoshop-keyboard-shortcuts><command <tool >< </command>",
            "<photoshop-keyboard-shortcuts><command kind=\"x\" id=\"9999999999999\">",
        ] {
            let _ = parse(junk);
        }
        for k in ["+", "++", "Cmd+", "+Cmd", "Shift+Shift+A", "Cmd+Opt+Control+Shift+F12", "é"] {
            let _ = ps_key(k);
        }
    }
}
