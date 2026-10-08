use super::*;
use photocraft_psd::abr::{AbrSample, write_v6};
use photocraft_psd::descriptor::{Descriptor, UnicodeString, Value as DV};

/// A complete-looking shortcut file: Photoshop defaults for the mapped table, then the changes
/// Bryan made (Cmd+E quick export, Cmd+X Reveal All, Cut on F2 only, Merge Layers and Camera Raw
/// unbound, F-key extras).
fn kys_sample() -> String {
    let mut x = String::from("<?xml version=\"1.0\"?>\n<photoshop-keyboard-shortcuts version=\"4\">\n");
    let ps = |k: &str| k.replace("Cmd+Alt+", "Opt+Cmd+").replace("Alt+", "Opt+");
    for (id, _, def) in kys::STATIC {
        let keys: Vec<String> = match *id {
            103 => vec!["F2".into()],
            104 => vec!["Cmd+C".into(), "F3".into()],
            1157 => vec!["Cmd+X".into()],
            3444 => vec!["Cmd+E".into()],
            1166 => continue,
            _ => match def {
                Some(d) => vec![ps(d)],
                None => continue,
            },
        };
        let name = if *id == 3444 { "Quick Export as JPG" } else { "x" };
        x.push_str(&format!("<command kind=\"static\" id=\"{id}\" name=\"{name}\">"));
        for k in keys {
            x.push_str(&format!("<shortcut>{k}</shortcut>"));
        }
        x.push_str("</command>\n");
    }
    x.push_str("<command kind=\"dynamic\" name=\"Liquify\"><shortcut>Shift+Cmd+X</shortcut></command>\n");
    x.push_str("<command kind=\"static\" id=\"9999\" name=\"Generative Thing...\"><shortcut>Opt+Shift+Cmd+G</shortcut></command>\n");
    x.push_str("<tool name=\"Brush Tool\" type=\"1\" key=\"1\">B</tool><tool name=\"Sponge Tool\" type=\"1\" key=\"2\">K</tool>\n");
    x.push_str("<tool name=\"Remove Tool\" type=\"1\" key=\"3\">J</tool><tool name=\"Default Foreground/Background Colors\" type=\"2\">D</tool>\n");
    x.push_str("</photoshop-keyboard-shortcuts>\n");
    x
}

#[test]
fn kys_import_applies_his_changes() {
    let mut s = Session::new();
    let r = s.execute("edit.keyboardShortcuts.importKys", json!({"text": kys_sample()})).unwrap();
    let prefs = s.prefs();
    assert_eq!(prefs.shortcuts.get("file.export.quickExportAsPng").map(String::as_str), Some("Cmd+E"));
    assert_eq!(prefs.shortcuts.get("image.revealAll").map(String::as_str), Some("Cmd+X"));
    assert_eq!(prefs.shortcuts.get("edit.cut").map(String::as_str), Some("F2"));
    assert_eq!(prefs.shortcuts.get("layer.mergeLayers").map(String::as_str), Some(""), "Merge Layers loses Cmd+E");
    assert_eq!(prefs.shortcuts.get("filter.cameraRaw").map(String::as_str), Some(""), "left out = unbound");
    assert!(!prefs.shortcuts.contains_key("edit.copy"), "unchanged main key: no override");
    assert_eq!(prefs.extra_shortcuts.get("edit.copy"), Some(&vec!["F3".to_string()]));
    assert_eq!(prefs.tool_keys.get("Sponge Tool").map(String::as_str), Some("K"));
    assert!(!prefs.tool_keys.contains_key("Brush Tool"));
    assert_eq!(serde_json::to_value(prefs.export.quick_export_format).unwrap(), "jpg");
    assert!(r["unmapped"].to_string().contains("Generative Thing"), "{r}");
    assert!(r["unmappedTools"].to_string().contains("Remove Tool"), "{r}");
    // Persisted with the preferences and in the keymap file split.
    let saved = s.prefs_to_json();
    let mut t = Session::new();
    t.load_prefs_json(&saved).unwrap();
    assert_eq!(t.prefs().extra_shortcuts, s.prefs().extra_shortcuts);
    // Importing again changes nothing.
    let before = s.prefs().clone();
    s.execute("edit.keyboardShortcuts.importKys", json!({"text": kys_sample()})).unwrap();
    assert_eq!(*s.prefs(), before);
    // Dry run applies nothing.
    let mut d = Session::new();
    let r = d.execute("edit.keyboardShortcuts.importKys", json!({"text": kys_sample(), "dryRun": true})).unwrap();
    assert!(!r["changed"].as_array().unwrap().is_empty());
    assert!(d.prefs().shortcuts.is_empty());
}

#[test]
fn kys_import_rejects_bad_input() {
    let mut s = Session::new();
    for p in [json!({}), json!({"text": "nope"}), json!({"path": "/no/such.kys"}), json!({"data": "%%"}), json!({"text": 3})] {
        assert!(s.execute("edit.keyboardShortcuts.importKys", p.clone()).is_err(), "{p}");
    }
    assert!(s.prefs().shortcuts.is_empty());
}

fn atn_bytes() -> Vec<u8> {
    let mut b = Vec::new();
    let u32b = |b: &mut Vec<u8>, v: u32| b.extend_from_slice(&v.to_be_bytes());
    let ustr = |b: &mut Vec<u8>, s: &str| {
        let u: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        b.extend_from_slice(&(u.len() as u32).to_be_bytes());
        for c in u {
            b.extend_from_slice(&c.to_be_bytes());
        }
    };
    let step = |b: &mut Vec<u8>, ev: &str, d: Option<Descriptor>| {
        b.extend_from_slice(&[0, 1, 0, 0]);
        b.extend_from_slice(b"TEXT");
        b.extend_from_slice(&(ev.len() as u32).to_be_bytes());
        b.extend_from_slice(ev.as_bytes());
        b.extend_from_slice(&(ev.len() as u32).to_be_bytes());
        b.extend_from_slice(ev.as_bytes());
        match d {
            Some(d) => {
                b.extend_from_slice(&(-1i32).to_be_bytes());
                b.extend_from_slice(&d.to_bytes());
            }
            None => b.extend_from_slice(&0i32.to_be_bytes()),
        }
    };
    u32b(&mut b, 16);
    u32b(&mut b, 1);
    ustr(&mut b, "NOISE GRAPHICS");
    b.push(0);
    u32b(&mut b, 2);
    for (name, steps) in [("Blur and invert", 2u32), ("Select stuff", 1)] {
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        ustr(&mut b, name);
        b.push(0);
        u32b(&mut b, steps);
        if steps == 2 {
            step(&mut b, "gaussianBlur", Some(Descriptor::new("GsnB").with("Rds ", DV::UnitFloat { unit: *b"#Pxl", value: 3.0 })));
            step(&mut b, "invert", None);
        } else {
            step(&mut b, "select", Some(Descriptor::new("slct")));
        }
    }
    b
}

#[test]
fn atn_import_runs_supported_steps_and_lists_the_rest() {
    let mut s = Session::new();
    let data = photocraft_paint::tile::b64_encode(&atn_bytes());
    let r = s.execute("actions.importAtn", json!({"data": data})).unwrap();
    assert_eq!(r["actions"][0]["runnable"], 2, "{r}");
    assert_eq!(r["actions"][1]["unsupported"][0], "select", "{r}");
    s.execute("file.new", json!({"width": 16, "height": 16})).unwrap();
    s.execute("layer.new.layer", json!({})).unwrap();
    let played = s.execute("actions.play", json!({"action": "Blur and invert"})).unwrap();
    assert_eq!(played["ran"], 2, "{played}");
    assert!(s.execute("actions.play", json!({"action": "Select stuff"})).is_err(), "unsupported steps refuse to play");
    assert_eq!(s.execute("actions.play", json!({"action": "Select stuff", "skipUnsupported": true})).unwrap()["ran"], 0);
    let listed = s.execute("actions.list", json!({})).unwrap();
    assert_eq!(listed["actions"][0]["set"], "NOISE GRAPHICS");
    // Re-import replaces instead of duplicating.
    s.execute("actions.importAtn", json!({"data": data})).unwrap();
    assert_eq!(s.actions.list.len(), 2);
    for p in [json!({}), json!({"data": "AAAA"}), json!({"path": "/no/such.atn"})] {
        assert!(s.execute("actions.importAtn", p.clone()).is_err(), "{p}");
    }
}

fn aco_bytes() -> Vec<u8> {
    let cols: [(u16, [u16; 4], &str); 3] =
        [(0, [0xffff, 0, 0, 0], "RGB Red"), (0, [0x3d3d, 0x4646, 0x3333, 0], "bwald"), (0, [0xf8f8, 0xf2f2, 0xdada, 0], "bwald2")];
    let mut b = vec![0, 1, 0, 3];
    for (sp, v, _) in cols {
        b.extend_from_slice(&sp.to_be_bytes());
        v.iter().for_each(|x| b.extend_from_slice(&x.to_be_bytes()));
    }
    b.extend_from_slice(&[0, 2, 0, 3]);
    for (sp, v, n) in cols {
        b.extend_from_slice(&sp.to_be_bytes());
        v.iter().for_each(|x| b.extend_from_slice(&x.to_be_bytes()));
        let u: Vec<u16> = n.encode_utf16().chain(std::iter::once(0)).collect();
        b.extend_from_slice(&(u.len() as u32).to_be_bytes());
        u.iter().for_each(|x| b.extend_from_slice(&x.to_be_bytes()));
    }
    b
}

/// A Brushes.psp: two presets, one in Photoshop's stock "General Brushes" group, one in "Mine".
fn abr_bytes() -> Vec<u8> {
    let preset = |n: &str| {
        Descriptor::new("brushPreset")
            .with("Nm  ", DV::Text(UnicodeString::new_nul(n)))
            .with("Brsh", DV::Descriptor(Descriptor::new("sampledBrush").with("sampledData", DV::Text(UnicodeString::new_nul("$s")))))
    };
    let tip = AbrSample { id: "$s".into(), width: 5, height: 5, depth: 16, data: vec![0x80; 50] };
    let mut b = write_v6(2, &[tip], &[], &[preset("Soft Round"), preset("Grit")], true).unwrap();
    let grp = |n: &str| DV::Descriptor(Descriptor::new("Grup").with("Nm  ", DV::Text(UnicodeString::new_nul(n))));
    let obj = |c: &str| DV::Descriptor(Descriptor::new(c));
    let h = photocraft_psd::VersionedDescriptor::new(
        Descriptor::new("null")
            .with("hierarchy", DV::List(vec![grp("General Brushes"), obj("preset"), obj("groupEnd"), grp("Mine"), obj("preset"), obj("groupEnd")])),
    )
    .to_bytes();
    b.extend_from_slice(b"8BIMphry");
    b.extend_from_slice(&(h.len() as u32).to_be_bytes());
    b.extend_from_slice(&h);
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
    b
}

fn pat_bytes() -> Vec<u8> {
    let mut lib = crate::pattern_cmds::builtin();
    let mut a = lib.remove(0);
    a.name = "fireflour".into();
    a.id = "fire-id".into();
    let mut b = lib.remove(0);
    b.name = "Tree Tile 4".into();
    b.id = "tree-id".into();
    photocraft_io::pattern_map::write_pat(&[a, b]).unwrap()
}

fn settings_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pc-ps-migrate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("guides")).unwrap();
    std::fs::write(dir.join("Keyboard Shortcuts.psp"), kys_sample()).unwrap();
    std::fs::write(dir.join("Actions Palette.psp"), atn_bytes()).unwrap();
    std::fs::write(dir.join("Swatches.psp"), aco_bytes()).unwrap();
    std::fs::write(dir.join("Brushes.psp"), abr_bytes()).unwrap();
    std::fs::write(dir.join("Patterns.psp"), pat_bytes()).unwrap();
    let gds = photocraft_psd::VersionedDescriptor::new(
        Descriptor::new("null").with("guideLayout", DV::Descriptor(Descriptor::new("guideLayout").with("colCount", DV::Integer(10)))),
    )
    .to_bytes();
    std::fs::write(dir.join("guides/insta 10.gds"), gds).unwrap();
    dir
}

#[test]
fn migration_imports_everything_but_stock_and_persists() {
    let dir = settings_dir("all");
    let store_dir = dir.join("PhotoCraftPresets");
    let mut s = Session::new();
    s.attach_preset_store(crate::preset_store::open_dir(&store_dir));
    let guides = vec![dir.join("guides/insta 10.gds").to_string_lossy().to_string()];
    let r = s.execute("file.migrateFromPhotoshop", json!({"settingsDir": dir.to_string_lossy(), "guideFiles": guides})).unwrap();
    assert_eq!(r["errors"], json!([]), "{r}");
    assert_eq!(r["swatches"]["count"], 2, "{r}");
    assert_eq!(r["brushes"]["count"], 1, "{r}");
    assert_eq!(r["brushes"]["skippedStock"], 1, "{r}");
    assert_eq!(r["patterns"]["count"], 1, "{r}");
    assert_eq!(r["actions"]["count"], 2, "{r}");
    assert_eq!(r["guides"]["count"], 1, "{r}");
    assert_eq!(s.prefs().shortcuts.get("file.export.quickExportAsPng").map(String::as_str), Some("Cmd+E"));
    assert!(s.tools.presets.iter().any(|p| p.name == "Grit" && p.group == "Mine"));
    assert!(!s.tools.presets.iter().any(|p| p.group == "General Brushes"));
    assert_eq!(s.presets.swatches.iter().flat_map(|g| &g.items).map(|w| w.name.as_str()).collect::<Vec<_>>(), vec!["bwald", "bwald2"]);
    s.sync_preset_store();
    // A new session on the same store and preferences sees it all.
    let saved = s.prefs_to_json();
    let mut t = Session::new();
    t.load_prefs_json(&saved).unwrap();
    t.attach_preset_store(crate::preset_store::open_dir(&store_dir));
    assert!(t.patterns.items.iter().any(|p| p.name == "fireflour"), "patterns persist in the store");
    assert!(t.tools.presets.iter().any(|p| p.name == "Grit"));
    assert_eq!(t.actions.list.len(), 2);
    assert_eq!(t.presets.swatches.len(), 1);
    assert!(crate::presets::swatches::guide_layout(&t, "insta 10").is_some());
    // Again: nothing doubles.
    s.execute("file.migrateFromPhotoshop", json!({"settingsDir": dir.to_string_lossy(), "guideFiles": []})).unwrap();
    assert_eq!(s.tools.presets.iter().filter(|p| p.name.starts_with("Grit")).count(), 1);
    assert_eq!(s.actions.list.len(), 2);
    assert_eq!(s.presets.swatches[0].items.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn migration_dry_run_and_stock_and_bad_params() {
    let dir = settings_dir("dry");
    let mut s = Session::new();
    let n = s.tools.presets.len();
    let r =
        s.execute("file.migrateFromPhotoshop", json!({"settingsDir": dir.to_string_lossy(), "dryRun": true, "includeStock": true, "guideFiles": []})).unwrap();
    assert_eq!(r["brushes"]["count"], 2, "{r}");
    assert_eq!(r["patterns"]["count"], 2, "{r}");
    assert_eq!(r["swatches"]["count"], 3, "{r}");
    assert_eq!(s.tools.presets.len(), n, "dry run changes nothing");
    assert!(s.actions.list.is_empty());
    assert!(s.prefs().shortcuts.is_empty());
    for p in [
        json!({"settingsDir": "/no/such/dir"}),
        json!({"settingsDir": 4}),
        json!({"settingsDir": dir.to_string_lossy(), "only": ["fonts"]}),
        json!({"settingsDir": dir.to_string_lossy(), "only": "x"}),
    ] {
        assert!(s.execute("file.migrateFromPhotoshop", p.clone()).is_err(), "{p}");
    }
    // A damaged file is reported, the rest still imports.
    std::fs::write(dir.join("Swatches.psp"), b"\x00\x09junk").unwrap();
    std::fs::write(dir.join("Actions Palette.psp"), b"").unwrap();
    let r = s.execute("file.migrateFromPhotoshop", json!({"settingsDir": dir.to_string_lossy(), "guideFiles": []})).unwrap();
    assert_eq!(r["errors"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(r["brushes"]["count"], 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The real thing, read-only, into a throwaway session (no store): set PHOTOCRAFT_PS_SETTINGS to
/// a Photoshop settings folder and run with `--ignored --nocapture`.
#[test]
#[ignore]
fn real_photoshop_settings_dry_run() {
    let Some(dir) = std::env::var_os("PHOTOCRAFT_PS_SETTINGS") else { return };
    let mut s = Session::new();
    let r = s.execute("file.migrateFromPhotoshop", json!({"settingsDir": dir, "dryRun": false})).unwrap();
    println!("{}", serde_json::to_string_pretty(&r).unwrap());
    assert_eq!(s.prefs().shortcuts.get("file.export.quickExportAsPng").map(String::as_str), Some("Cmd+E"));
}

#[test]
fn keymap_file_round_trips_and_ignores_junk() {
    let mut s = Session::new();
    s.execute("edit.keyboardShortcuts.importKys", json!({"text": kys_sample()})).unwrap();
    s.edit_prefs(|p| {
        p.mouse_buttons.insert("MouseBack".into(), "edit.undo".into());
    });
    let saved: Value = serde_json::from_str(&s.prefs_to_json()).unwrap();
    let km = crate::prefs::keymap_of(&saved);
    assert_eq!(km["shortcuts"]["edit.cut"], "F2");
    assert_eq!(km["mouseButtons"]["MouseBack"], "edit.undo");
    let mut fresh: Value = serde_json::from_str(&Session::new().prefs_to_json()).unwrap();
    crate::prefs::merge_keymap(&mut fresh, &km);
    let mut t = Session::new();
    t.load_prefs_json(&fresh.to_string()).unwrap();
    assert_eq!(t.prefs().shortcuts, s.prefs().shortcuts);
    assert_eq!(t.prefs().extra_shortcuts, s.prefs().extra_shortcuts);
    assert_eq!(t.prefs().mouse_buttons, s.prefs().mouse_buttons);
    for junk in [json!("x"), json!(null), json!({"shortcuts": 3}), json!([1, 2])] {
        let mut v = fresh.clone();
        crate::prefs::merge_keymap(&mut v, &junk);
        assert!(Session::new().load_prefs_json(&v.to_string()).is_ok(), "{junk}");
    }
    let mut not_obj = json!(7);
    crate::prefs::merge_keymap(&mut not_obj, &km);
    assert!(not_obj.is_object());
}
