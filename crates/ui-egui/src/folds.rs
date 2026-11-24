//! Foldable lists and sections (Bryan, 2026-11-23: "Every list or section of items in his apps
//! must be collapsible (tap the header to fold / unfold, remembered)").
//!
//! One map, `Preferences::folds` (section id → open, saved with the preferences), holds every
//! fold. Drawing code only has a `Ui`, so the map is mirrored into the egui context each frame
//! ([`sync`], at the top of the app's `ui`): a header click is queued there and written to the
//! preferences on the next frame; the `ui.fold` / `ui.folds` commands change both at once.
//! Sections are open until folded, so nothing looks different until Bryan folds one.
//!
//! Section ids are stable, dotted and lowercase (`properties.transform`, `export.file.settings`,
//! `swatches.group:Pastels`). Every header's accessible label is its title, so tests and agents
//! find it by name.

use std::collections::BTreeMap;

use egui::{Rect, Response, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::PhotocraftApp;
use crate::theme::{self, Tokens};

#[derive(Clone, Default)]
struct Live {
    map: BTreeMap<String, bool>,
    /// Header clicks this frame, written to the preferences by the next [`sync`].
    queued: Vec<(String, bool)>,
    /// Headers drawn last frame (id, title), for `ui.folds`.
    drawn: Vec<(u64, String, String)>,
}

fn data_id() -> egui::Id {
    egui::Id::new("photocraft.folds")
}

/// Writes last frame's header clicks into the preferences and mirrors their fold map into the
/// context for this frame's drawing.
pub fn sync(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let live: Live = ctx.data(|d| d.get_temp(data_id())).unwrap_or_default();
    if !live.queued.is_empty() {
        app.session.edit_prefs(|p| {
            for (id, open) in &live.queued {
                p.folds.insert(id.clone(), *open);
            }
        });
    }
    let map = app.session.prefs().folds.clone();
    ctx.data_mut(|d| d.insert_temp(data_id(), Live { map, queued: Vec::new(), drawn: live.drawn }));
}

/// Whether section `id` is open this frame (never folded: open).
pub fn is_open(ctx: &egui::Context, id: &str) -> bool {
    is_open_or(ctx, id, true)
}

/// Whether section `id` is open this frame; `default` until it is first folded or unfolded.
pub fn is_open_or(ctx: &egui::Context, id: &str, default: bool) -> bool {
    ctx.data(|d| d.get_temp::<Live>(data_id()).and_then(|l| l.map.get(id).copied())).unwrap_or(default)
}

/// Opens or closes `id` now (this frame) and in the preferences from the next frame on.
pub fn set(ctx: &egui::Context, id: &str, open: bool) {
    ctx.data_mut(|d| {
        let live = d.get_temp_mut_or_default::<Live>(data_id());
        live.map.insert(id.to_string(), open);
        live.queued.push((id.to_string(), open));
    });
    ctx.request_repaint();
}

fn note_drawn(ctx: &egui::Context, id: &str, title: &str) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let live = d.get_temp_mut_or_default::<Live>(data_id());
        live.drawn.retain(|(p, _, _)| *p + 1 >= pass);
        if !live.drawn.iter().any(|(p, i, _)| *p == pass && i == id) {
            live.drawn.push((pass, id.to_string(), title.to_string()));
        }
    });
}

/// Folds `id` when `resp` (a header the caller drew) was clicked; returns whether it is open now.
pub fn toggle_on_click(ui: &Ui, resp: &Response, id: &str) -> bool {
    toggle_on_click_or(ui, resp, id, true)
}

/// [`toggle_on_click`] for a section that starts `default` (open or closed).
pub fn toggle_on_click_or(ui: &Ui, resp: &Response, id: &str, default: bool) -> bool {
    let open = is_open_or(ui.ctx(), id, default);
    if resp.clicked() {
        set(ui.ctx(), id, !open);
        return !open;
    }
    open
}

/// The ▾ / ▸ chevron, centered at `c`.
pub fn paint_chevron(ui: &Ui, c: egui::Pos2, open: bool, color: egui::Color32) {
    crate::icons::paint(ui, Rect::from_center_size(c, vec2(12.0, 12.0)), if open { "chevron-down" } else { "chevron-right" }, 11.0, color);
}

/// A foldable header over the full width: chevron + `title` (semibold 12 pt), 24 points high.
/// Click anywhere on it to fold or unfold. Returns whether the section is open.
pub fn header(ui: &mut Ui, id: &str, title: &str) -> bool {
    header_or(ui, id, title, true)
}

/// [`header`] for a section that starts `default` (open or closed).
pub fn header_or(ui: &mut Ui, id: &str, title: &str, default: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(40.0), 24.0), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let open = toggle_on_click_or(ui, &resp, id, default);
    paint_chevron(ui, pos2(r.left() + 7.0, r.center().y), open, t.text_dim);
    let clip = Rect::from_min_max(pos2(r.left() + 18.0, r.top()), r.right_bottom());
    ui.painter().with_clip_rect(clip).text(pos2(r.left() + 18.0, r.center().y), egui::Align2::LEFT_CENTER, tl!(title), theme::semibold(12.0), t.text);
    let label = title.to_string();
    resp.widget_info(move || egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, &label));
    note_drawn(ui.ctx(), id, title);
    open
}

/// A foldable small-caps group label ([`crate::widgets::section_label`]'s look with a chevron),
/// as wide as its text. Returns whether the group is open.
pub fn label(ui: &mut Ui, id: &str, title: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    label_with(ui, id, title, theme::medium(11.5), t.text_faint)
}

/// [`label`] in another font and colour (a group title in a list: semibold, the text colour).
pub fn label_with(ui: &mut Ui, id: &str, title: &str, font: egui::FontId, color: egui::Color32) -> bool {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(tl!(title).to_string(), font, color);
    let (r, resp) = ui.allocate_exact_size(vec2(galley.size().x + 18.0, galley.size().y.max(18.0)), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let open = toggle_on_click(ui, &resp, id);
    paint_chevron(ui, pos2(r.left() + 6.0, r.center().y), open, if resp.hovered() { t.text } else { t.text_faint });
    ui.painter().galley(pos2(r.left() + 18.0, r.center().y - galley.size().y / 2.0), galley, color);
    let label = title.to_string();
    resp.widget_info(move || egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, &label));
    note_drawn(ui.ctx(), id, title);
    open
}

/// Width of a [`chevron`] (spacers beside headings without one keep them in line).
pub const CHEVRON_W: f32 = 16.0;

/// Just the chevron, for a heading that is a control of its own (a checkbox, a set's row):
/// [`CHEVRON_W`] × `h`; a click folds `id`. Returns whether the section is open.
pub fn chevron(ui: &mut Ui, id: &str, title: &str, h: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(CHEVRON_W, h), Sense::click());
    let open = toggle_on_click(ui, &resp, id);
    paint_chevron(ui, r.center(), open, if resp.hovered() { t.text } else { t.text_dim });
    let label = format!("fold {title}");
    resp.widget_info(move || egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, true, open, &label));
    note_drawn(ui.ctx(), id, title);
    open
}

/// `Image Size` → `image.size`: a title as a fold id part.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for w in title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push('.');
        }
        out.push_str(&w.to_lowercase());
    }
    out
}

/// `ui.fold {id, open?}`: opens or closes a section (no `open`: toggles it). `ui.folds`: the saved
/// folds and the foldable sections drawn last frame.
pub fn invoke(app: &mut PhotocraftApp, ctx: &egui::Context, id: &str, p: &Value) -> Option<Result<Value, String>> {
    match id {
        "ui.fold" => {
            let Some(section) = p.get("id").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) else {
                return Some(Err("ui.fold needs `id`, the section to fold (see ui.folds)".into()));
            };
            let open = match p.get("open") {
                None | Some(Value::Null) => !app.session.prefs().folds.get(section).copied().unwrap_or(true),
                Some(v) => match v.as_bool() {
                    Some(b) => b,
                    None => return Some(Err("`open` must be true or false".into())),
                },
            };
            let key = section.to_string();
            app.session.edit_prefs(|prefs| prefs.folds.insert(key, open));
            ctx.data_mut(|d| {
                d.get_temp_mut_or_default::<Live>(data_id()).map.insert(section.to_string(), open);
            });
            ctx.request_repaint();
            Some(Ok(json!({"id": section, "open": open})))
        }
        "ui.folds" => {
            let drawn: Vec<(u64, String, String)> = ctx.data(|d| d.get_temp::<Live>(data_id()).map(|l| l.drawn)).unwrap_or_default();
            let folds = &app.session.prefs().folds;
            let mut seen = std::collections::BTreeSet::new();
            let shown: Vec<Value> = drawn
                .iter()
                .filter(|(_, id, _)| seen.insert(id.clone()))
                .map(|(_, id, title)| json!({"id": id, "title": title, "open": folds.get(id).copied().unwrap_or(true)}))
                .collect();
            let folded: Vec<&String> = folds.iter().filter(|(_, open)| !**open).map(|(k, _)| k).collect();
            Some(Ok(json!({"folded": folded, "saved": folds, "onScreen": shown})))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use crate::PhotocraftApp;

    /// An app whose preferences live in `store` (as on disk).
    fn app_on(store: &Arc<Mutex<Option<String>>>) -> PhotocraftApp {
        let (a, b) = (store.clone(), store.clone());
        let services = crate::Services {
            load_prefs: Some(Box::new(move || a.lock().unwrap().clone())),
            save_prefs: Some(Box::new(move |s: &str| {
                *b.lock().unwrap() = Some(s.to_string());
                Ok(())
            })),
            ..Default::default()
        };
        PhotocraftApp::new(photocraft_engine::Session::new(), services)
    }

    #[test]
    fn a_fold_is_a_command_and_survives_saving_and_loading_the_preferences() {
        let store = Arc::new(Mutex::new(None));
        let ctx = egui::Context::default();
        let mut app = app_on(&store);
        crate::prefs_ui::load(&mut app);
        let r = crate::menus::invoke(&mut app, &ctx, "ui.fold", json!({"id": "export.as.file.settings", "open": false})).unwrap();
        assert_eq!(r, json!({"id": "export.as.file.settings", "open": false}));
        // no `open`: toggles
        assert_eq!(crate::menus::invoke(&mut app, &ctx, "ui.fold", json!({"id": "print.printer.setup"})).unwrap()["open"], false);
        assert_eq!(crate::menus::invoke(&mut app, &ctx, "ui.fold", json!({"id": "print.printer.setup"})).unwrap()["open"], true);
        let r = crate::menus::invoke(&mut app, &ctx, "ui.folds", json!({})).unwrap();
        assert_eq!(r["folded"], json!(["export.as.file.settings"]), "{r}");
        // bad params are errors, never a crash
        for bad in [json!({}), json!({"id": "  "}), json!({"id": "x", "open": 1})] {
            assert!(crate::menus::invoke(&mut app, &ctx, "ui.fold", bad).is_err());
        }
        // written with the preferences…
        crate::prefs_ui::tick(&mut app, &ctx);
        let saved: serde_json::Value = serde_json::from_str(store.lock().unwrap().as_deref().unwrap()).unwrap();
        assert_eq!(saved["folds"]["export.as.file.settings"], false, "{saved}");
        // …and read back by the next launch
        let mut next = app_on(&store);
        crate::prefs_ui::load(&mut next);
        assert_eq!(next.session.prefs().folds.get("export.as.file.settings"), Some(&false));
        assert_eq!(next.session.prefs().folds.get("print.printer.setup"), Some(&true));
        // preferences from before folds existed: every section open
        let old: photocraft_engine::prefs::Preferences = serde_json::from_value(json!({"general": {}})).unwrap();
        assert!(old.folds.is_empty());
    }

    fn harness() -> Harness<'static, PhotocraftApp> {
        let mut h = Harness::builder().with_size(vec2(1440.0, 900.0)).with_max_steps(64).build_eframe(|cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
            app.run("file.new", json!({"width": 200, "height": 120})).unwrap();
            app
        });
        h.run_steps(6);
        h
    }

    /// Folded sections hide what is in them: a header click (Export As ▸ File Settings), and the
    /// command (Print ▸ Printer Setup); the click lands in the preferences.
    #[test]
    fn folded_sections_hide_their_contents() {
        let mut h = harness();
        crate::export_dialog::open(h.state_mut()).unwrap();
        h.run_steps(6);
        assert!(h.query_by_label("Format").is_some(), "File Settings open");
        assert!(h.query_by_label("Scale").is_some(), "Image Size open");
        h.get_by_label("File Settings").click();
        h.run_steps(6);
        assert!(h.query_by_label("Format").is_none(), "File Settings folded");
        assert!(h.query_by_label("Scale").is_some(), "Image Size still open");
        assert_eq!(h.state().session.prefs().folds.get("export.as.file.settings"), Some(&false), "remembered");
        let ctx = h.ctx.clone();
        let r = crate::menus::invoke(h.state_mut(), &ctx, "ui.folds", json!({})).unwrap();
        assert!(r["onScreen"].as_array().unwrap().iter().any(|s| s["id"] == "export.as.image.size" && s["open"] == true), "{r}");
        h.get_by_label("File Settings").click();
        h.run_steps(6);
        assert!(h.query_by_label("Format").is_some(), "unfolded again");
        h.state_mut().ui.dialogs.clear();
        h.run_steps(4);
        // Print ▸ Printer Setup, folded by the command
        crate::file_ui::open_print(h.state_mut());
        h.run_steps(6);
        assert!(h.query_by_label("Copies").is_some(), "Printer Setup open");
        crate::menus::invoke(h.state_mut(), &ctx, "ui.fold", json!({"id": "print.printer.setup", "open": false})).unwrap();
        h.run_steps(6);
        assert!(h.query_by_label("Copies").is_none(), "Printer Setup folded");
        assert!(h.query_by_label("Color Management").is_some(), "the next group's title stays");
    }
}
