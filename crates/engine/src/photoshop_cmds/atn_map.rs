//! Photoshop action steps → PhotoCraft commands.
//!
//! Only steps whose meaning is plain from their event id and a few numeric parameters map
//! (filters, adjustments, selection basics). Steps that address layers, channels or documents by
//! reference (`select`, `set`, `make`, `move`, `delete`…, the bulk of most recorded actions) need
//! an ActionDescriptor interpreter PhotoCraft doesn't have yet; they are reported as unsupported
//! rather than guessed at.

use photocraft_psd::atn::AtnStep;
use photocraft_psd::descriptor::{Descriptor, ReferenceItem, Value as D};
use serde_json::{Value, json};

/// What a Photoshop step becomes.
#[derive(Clone, Debug, PartialEq)]
pub enum Mapped {
    /// A PhotoCraft command with params.
    Run(&'static str, Value),
    /// A step with no effect on the image (history snapshots): dropped.
    Skip,
    /// No PhotoCraft equivalent yet.
    Unsupported,
}

fn num(d: Option<&Descriptor>, k: &str) -> Option<f64> {
    match d?.get(k)? {
        D::Double(v) => Some(*v),
        D::UnitFloat { value, .. } => Some(*value),
        D::Integer(i) => Some(f64::from(*i)),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn enum_value(d: Option<&Descriptor>, k: &str) -> Option<String> {
    match d?.get(k)? {
        D::Enumerated { value, .. } => Some(String::from_utf8_lossy(value.as_bytes()).to_string()),
        _ => None,
    }
}

fn bool_of(d: Option<&Descriptor>, k: &str) -> Option<bool> {
    match d?.get(k)? {
        D::Boolean(b) => Some(*b),
        _ => None,
    }
}

/// `null` reference's first item class (`Lyr `, `Chnl`, `SnpS`…) and property key.
fn target(d: Option<&Descriptor>) -> (String, String) {
    let Some(D::Reference(items)) = d.and_then(|d| d.get("null")) else { return Default::default() };
    match items.first() {
        Some(ReferenceItem::Class(c)) => (String::from_utf8_lossy(c.class_id.as_bytes()).to_string(), String::new()),
        Some(ReferenceItem::Property { class, key }) => {
            (String::from_utf8_lossy(class.class_id.as_bytes()).to_string(), String::from_utf8_lossy(key.as_bytes()).to_string())
        }
        Some(ReferenceItem::Enumerated { class, .. }) => (String::from_utf8_lossy(class.class_id.as_bytes()).to_string(), String::new()),
        _ => Default::default(),
    }
}

/// Map one step.
pub fn map_step(step: &AtnStep) -> Mapped {
    let d = step.descriptor.as_ref();
    let has = |k: &str| d.is_some_and(|d| d.get(k).is_some());
    match step.event.trim() {
        "copyToLayer" | "CpTL" => Mapped::Run("layer.new.layerViaCopy", json!({})),
        "cutToLayer" | "CtTL" => Mapped::Run("layer.new.layerViaCut", json!({})),
        "invert" | "Invr" => Mapped::Run("image.adjustments.invert", json!({})),
        "desaturate" | "Dstt" => Mapped::Run("image.adjustments.desaturate", json!({})),
        "flattenImage" | "FltI" => Mapped::Run("layer.flattenImage", json!({})),
        "mergeVisible" | "MrgV" if !has("Dplc") => Mapped::Run("layer.mergeVisible", json!({})),
        "mergeVisible" | "MrgV" | "mergeLayersNew" => Mapped::Run("layer.stampVisible", json!({})),
        "clouds" | "Clds" => Mapped::Run("filter.render.clouds", json!({})),
        "findEdges" | "FndE" => Mapped::Run("filter.stylize.findEdges", json!({})),
        "rasterizeLayer" | "rasterizeTypeLayer" => Mapped::Run("layer.rasterize.layer", json!({})),
        "newPlacedLayer" => Mapped::Run("layer.smartObjects.convertToSmartObject", json!({})),
        "copyEvent" | "copy" => Mapped::Run("edit.copy", json!({})),
        "paste" | "past" if d.is_none() => Mapped::Run("edit.paste", json!({})),
        "crop" | "Crop" if d.is_none() => Mapped::Run("image.crop", json!({})),
        "inverse" | "Invs" => Mapped::Run("select.inverse", json!({})),
        "groupEvent" if step.name.contains("Clipping") => Mapped::Run("layer.createClippingMask", json!({})),
        "gaussianBlur" | "GsnB" => Mapped::Run("filter.blur.gaussianBlur", json!({"radius": num(d, "Rds ").unwrap_or(1.0)})),
        "highPass" | "HghP" => Mapped::Run("filter.other.highPass", json!({"radius": num(d, "Rds ").unwrap_or(10.0)})),
        "motionBlur" | "MtnB" => {
            Mapped::Run("filter.blur.motionBlur", json!({"angle": num(d, "Angl").unwrap_or(0.0), "distance": num(d, "Dstn").unwrap_or(10.0)}))
        }
        "unsharpMask" | "UnsM" => Mapped::Run(
            "filter.sharpen.unsharpMask",
            json!({"amount": num(d, "Amnt").unwrap_or(50.0), "radius": num(d, "Rds ").unwrap_or(1.0), "threshold": num(d, "Thsh").unwrap_or(0.0)}),
        ),
        "addNoise" | "AdNs" => Mapped::Run(
            "filter.noise.addNoise",
            json!({
                "amount": num(d, "Amnt").unwrap_or(12.5),
                "distribution": if enum_value(d, "Dstr").as_deref() == Some("Gsn ") { "gaussian" } else { "uniform" },
                "monochromatic": bool_of(d, "Mnch").unwrap_or(false),
            }),
        ),
        "feather" | "Fthr" => Mapped::Run("select.modify.feather", json!({"radius": num(d, "Rds ").unwrap_or(1.0)})),
        "contract" | "Cntc" => Mapped::Run("select.modify.contract", json!({"radius": num(d, "By  ").unwrap_or(1.0)})),
        "reset" | "Rset" if target(d).1 == "Clrs" => Mapped::Run("tools.defaultColors", json!({})),
        "make" | "Mk  " if target(d).0 == "SnpS" => Mapped::Skip,
        "make" | "Mk  " if target(d).0 == "Lyr " && !has("Usng") && !has("Nw  ") => Mapped::Run("layer.new.layer", json!({})),
        "set" | "setd" if target(d).1 == "fsel" => match d.and_then(|d| d.get("T   ")) {
            Some(D::Enumerated { value, .. }) if value.is("None") => Mapped::Run("select.deselect", json!({})),
            Some(D::Enumerated { value, .. }) if value.is("Al  ") => Mapped::Run("select.all", json!({})),
            _ => Mapped::Unsupported,
        },
        _ => Mapped::Unsupported,
    }
}

/// An imported action: runnable steps, the Photoshop steps PhotoCraft can't run, and every
/// Photoshop step name.
pub fn map_action(steps: &[AtnStep]) -> (Vec<(String, Value)>, Vec<String>, Vec<String>) {
    let mut run = Vec::new();
    let mut unsupported = Vec::new();
    let mut names = Vec::new();
    for s in steps {
        let label = if s.name.trim().is_empty() { s.event.clone() } else { s.name.trim().to_string() };
        names.push(if s.enabled { label.clone() } else { format!("{label} (off)") });
        if !s.enabled {
            continue;
        }
        match map_step(s) {
            Mapped::Run(id, p) => run.push((id.to_string(), p)),
            Mapped::Skip => {}
            Mapped::Unsupported => unsupported.push(label),
        }
    }
    (run, unsupported, names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_psd::descriptor::{Class, Id, UnicodeString};

    fn step(event: &str, d: Option<Descriptor>) -> AtnStep {
        AtnStep { event: event.into(), name: event.into(), enabled: true, with_dialog: false, descriptor: d }
    }

    #[test]
    fn maps_simple_steps_and_reports_the_rest() {
        let blur = Descriptor::new("GsnB").with("Rds ", D::UnitFloat { unit: *b"#Pxl", value: 2.8 });
        assert_eq!(map_step(&step("gaussianBlur", Some(blur))), Mapped::Run("filter.blur.gaussianBlur", json!({"radius": 2.8})));
        let snap =
            Descriptor::new("Mk  ").with("null", D::Reference(vec![ReferenceItem::Class(Class { name: UnicodeString::default(), class_id: Id::new("SnpS") })]));
        assert_eq!(map_step(&step("make", Some(snap))), Mapped::Skip);
        let none = Descriptor::new("setd")
            .with(
                "null",
                D::Reference(vec![ReferenceItem::Property {
                    class: Class { name: UnicodeString::default(), class_id: Id::new("Chnl") },
                    key: Id::new("fsel"),
                }]),
            )
            .with("T   ", D::Enumerated { type_id: Id::new("Ordn"), value: Id::new("None") });
        assert_eq!(map_step(&step("set", Some(none))), Mapped::Run("select.deselect", json!({})));
        assert_eq!(map_step(&step("select", None)), Mapped::Unsupported);
        let mut off = step("invert", None);
        off.enabled = false;
        let (run, unsupported, names) = map_action(&[step("invert", None), step("select", None), off]);
        assert_eq!(run.len(), 1);
        assert_eq!(unsupported, vec!["select"]);
        assert_eq!(names, vec!["invert", "select", "invert (off)"]);
    }

    #[test]
    fn mapped_ids_are_commands() {
        let d = Descriptor::new("x");
        for ev in [
            "copyToLayer",
            "cutToLayer",
            "invert",
            "desaturate",
            "flattenImage",
            "mergeVisible",
            "mergeLayersNew",
            "clouds",
            "findEdges",
            "rasterizeLayer",
            "newPlacedLayer",
            "copyEvent",
            "paste",
            "crop",
            "inverse",
            "gaussianBlur",
            "highPass",
            "motionBlur",
            "unsharpMask",
            "addNoise",
            "feather",
            "contract",
        ] {
            for desc in [None, Some(d.clone())] {
                if let Mapped::Run(id, _) = map_step(&step(ev, desc)) {
                    assert!(crate::commands::find(id).is_some(), "{ev} → {id} is not a command");
                }
            }
        }
    }
}
