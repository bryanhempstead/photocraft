//! UI text. PhotoCraft is English only (Bryan, 2026-11-23): strings in code are the display text.
//! The lookups below stay as the markers on display text, so their call sites are unchanged.
//!
//! - [`tr`] / [`t`] (`tl!`): a plain string. [`tr_ctx`]: one with a disambiguating context.
//! - [`tr_id`]: a command's label. [`trn`]: plural-aware (`{n}` is filled in). [`fmt`]: fill
//!   `{name}` placeholders.

/// The UI language: English, the only one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lang;

impl Lang {
    pub const EN: Lang = Lang;

    pub fn code(self) -> &'static str {
        "en"
    }
}

/// The language the UI is drawn in.
pub fn current() -> Lang {
    Lang::EN
}

/// A UI string as shown.
pub fn t(s: &str) -> &str {
    s
}

/// A UI string as shown.
pub fn tr(_lang: Lang, s: &str) -> &str {
    s
}

/// Like [`tr`], for a string with a disambiguating `context`.
pub fn tr_ctx<'a>(_lang: Lang, _context: &str, s: &'a str) -> &'a str {
    s
}

/// A command's label.
pub fn tr_id<'a>(_lang: Lang, _id: &str, label: &'a str) -> &'a str {
    label
}

/// Fill `{name}` placeholders. Unknown placeholders are left as written.
pub fn fmt(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// A plural-aware message: `one`/`other` are the forms (with `{n}` where the count goes).
pub fn trn(_lang: Lang, n: u64, one: &str, other: &str) -> String {
    fmt(if n == 1 { one } else { other }, &[("n", &n.to_string())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plurals_and_placeholders() {
        assert_eq!(trn(Lang::EN, 1, "{n} item", "{n} items"), "1 item");
        assert_eq!(trn(Lang::EN, 0, "{n} item", "{n} items"), "0 items");
        assert_eq!(trn(Lang::EN, 7, "{n} item", "{n} items"), "7 items");
        assert_eq!(fmt("{b} before {a}", &[("a", "x"), ("b", "y"), ("c", "z")]), "y before x");
        assert_eq!(fmt("{missing}", &[]), "{missing}");
        assert_eq!(tr(current(), "Layer"), "Layer");
    }

    /// Old language requests (the app menu's Language items, `interface.language`) answer with an
    /// error instead of doing nothing, and settings saved with a language still load.
    #[test]
    fn language_requests_fail_clearly() {
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        let ctx = egui::Context::default();
        let e = crate::menus::invoke(&mut app, &ctx, "app.language.fr", serde_json::json!({})).unwrap_err();
        assert!(e.contains("English only"), "{e}");
        let e = app.run("prefs.set", serde_json::json!({"values": {"interface.language": "fr"}})).unwrap_err();
        assert!(e.contains("English only"), "{e}");
        let old: photocraft_engine::prefs::Preferences = serde_json::from_str(r#"{"interface":{"language":"ja","showTooltips":false}}"#).unwrap();
        assert!(!old.interface.show_tooltips);
    }
}
