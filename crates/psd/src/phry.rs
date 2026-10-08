//! The `phry` preset hierarchy Photoshop 2020+ writes into preset files (`.abr`, `Swatches.psp`,
//! …): a versioned descriptor whose `hierarchy` list holds `Grup` (with `Nm  `), `preset` and
//! `groupEnd` objects in order. The n-th `preset` entry is the file's n-th preset, so the list
//! gives every preset its group.

use crate::descriptor::{Value, VersionedDescriptor};

/// Most nested group levels tracked (deeper levels keep their parent's path).
const MAX_DEPTH: usize = 32;

/// The group path of each preset in file order (`"Outer / Inner"`, empty for ungrouped presets).
/// Unreadable data yields an empty list (presets then keep their default group). Never panics.
pub fn groups(data: &[u8]) -> Vec<String> {
    let Ok((vd, _)) = VersionedDescriptor::parse_prefix(data) else { return Vec::new() };
    let Some(Value::List(items)) = vd.descriptor.get("hierarchy") else { return Vec::new() };
    let mut path: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for it in items {
        let Value::Descriptor(d) = it else { continue };
        if d.class_id.is("Grup") {
            let name = match d.get("Nm  ") {
                Some(Value::Text(t)) => crate::atn::zstring(&t.to_string_lossy()),
                _ => String::new(),
            };
            if path.len() < MAX_DEPTH {
                path.push(name);
            }
        } else if d.class_id.is("groupEnd") {
            path.pop();
        } else if d.class_id.is("preset") {
            out.push(path.iter().filter(|p| !p.is_empty()).cloned().collect::<Vec<_>>().join(" / "));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::{Descriptor, UnicodeString};

    fn obj(class: &str) -> Value {
        Value::Descriptor(Descriptor::new(class))
    }

    #[test]
    fn assigns_groups_in_order() {
        let grp = |n: &str| Value::Descriptor(Descriptor::new("Grup").with("Nm  ", Value::Text(UnicodeString::new_nul(n))));
        let list = vec![obj("preset"), grp("A"), obj("preset"), grp("B"), obj("preset"), obj("groupEnd"), obj("groupEnd"), obj("preset")];
        let d = VersionedDescriptor::new(Descriptor::new("null").with("hierarchy", Value::List(list)));
        let bytes = d.to_bytes();
        assert_eq!(groups(&bytes), vec!["", "A", "A / B", ""]);
        for cut in 0..bytes.len() {
            let _ = groups(&bytes[..cut]);
        }
        assert!(groups(b"junk").is_empty());
        // Unbalanced ends never underflow.
        let d = VersionedDescriptor::new(Descriptor::new("null").with("hierarchy", Value::List(vec![obj("groupEnd"), obj("preset")])));
        assert_eq!(groups(&d.to_bytes()), vec![""]);
    }
}
