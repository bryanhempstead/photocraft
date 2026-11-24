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
    let galley = ui.painter().layout_no_wrap(tl!(title).to_string(), theme::medium(11.5), t.text_faint);
    let (r, resp) = ui.allocate_exact_size(vec2(galley.size().x + 18.0, galley.size().y.max(18.0)), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let open = toggle_on_click(ui, &resp, id);
    paint_chevron(ui, pos2(r.left() + 6.0, r.center().y), open, if resp.hovered() { t.text } else { t.text_faint });
    ui.painter().galley(pos2(r.left() + 18.0, r.center().y - galley.size().y / 2.0), galley, t.text_faint);
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
