//! ps-compare: how far is PhotoCraft's colour from Photoshop's, measured on real PSDs.
//!
//! Every PSD/PSB saved with "Maximize Compatibility" carries Photoshop's own merged composite and
//! the document's ICC profile. For each file this tool renders the document from its LAYERS with
//! PhotoCraft (import + flatten, exactly what the app shows), takes Photoshop's stored composite,
//! puts both over white, converts both to CIE Lab (D50) through the document's profile with Little
//! CMS (an independent CMM, not PhotoCraft's), and reports CIEDE2000 per file and per feature
//! (each feature's statistics are over the pixels its layers cover).
//!
//! ```sh
//! cargo run --release -p photocraft-ps-compare -- [options] <file.psd|dir>...
//!   --out DIR          write report.json, report.md and per-file panels (ps.png, ours.png, de.png)
//!   --baseline FILE    an earlier report.json: print before → after per file and per feature
//!   --csf FILE         Photoshop Color Settings (.csf) whose working spaces tag untagged files
//!                      (default: the newest ~/Library/Preferences/Adobe Photoshop * Settings/Color Settingscsf;
//!                      sRGB when none)
//!   --max-mp N         skip documents above N megapixels (default 60)
//!   --panel-width N    width of each saved panel (default 900)
//! ```
//! Untagged RGB documents are measured in the working RGB space (Photoshop shows them that way
//! under "Preserve embedded profiles" with profile warnings off). CMYK, Lab and multichannel
//! documents are compared on the RGB both sides convert to (marked `approx`).
#![allow(clippy::too_many_arguments, clippy::type_complexity)] // a measurement tool, not product code

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lcms2::{Intent, PixelFormat, Profile, Transform};
use photocraft_color::{ColorMode, SampleType};
use photocraft_doc::{Document, Layer, LayerContent};
use photocraft_ps_compare::de2000;
use serde_json::{Value, json};

const HIST_BINS: usize = 4000; // 0.025 ΔE per bin up to 100
const HIST_STEP: f32 = 0.025;

#[derive(Clone, Default)]
struct Hist {
    bins: Vec<u64>,
    sum: f64,
    n: u64,
    max: f32,
}

impl Hist {
    fn new() -> Self {
        Self { bins: vec![0; HIST_BINS], ..Default::default() }
    }
    fn add(&mut self, d: f32) {
        let i = ((d / HIST_STEP) as usize).min(HIST_BINS - 1);
        self.bins[i] += 1;
        self.sum += f64::from(d);
        self.n += 1;
        self.max = self.max.max(d);
    }
    fn mean(&self) -> f64 {
        if self.n == 0 { 0.0 } else { self.sum / self.n as f64 }
    }
    fn pct(&self, q: f64) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        let target = (q * self.n as f64).ceil() as u64;
        let mut acc = 0;
        for (i, b) in self.bins.iter().enumerate() {
            acc += b;
            if acc >= target {
                return (i as f64 + 1.0) * f64::from(HIST_STEP);
            }
        }
        f64::from(self.max)
    }
    fn frac_over(&self, t: f32) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        let i = (t / HIST_STEP) as usize;
        let over: u64 = self.bins.iter().skip(i).sum();
        over as f64 / self.n as f64
    }
    fn json(&self) -> Value {
        json!({"pixels": self.n, "mean": r3(self.mean()), "p50": r3(self.pct(0.5)), "p95": r3(self.pct(0.95)), "p99": r3(self.pct(0.99)),
               "max": r3(f64::from(self.max)), "over2": r3(100.0 * self.frac_over(2.0)), "over5": r3(100.0 * self.frac_over(5.0))})
    }
}

fn r3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Photoshop's working spaces from a Color Settings (.csf) file: (RGB, Gray) ICC bytes.
fn csf_working(path: &Path) -> Option<(Option<Vec<u8>>, Option<Vec<u8>>)> {
    let d = std::fs::read(path).ok()?;
    let n = u32::from_be_bytes(d.get(128..132)?.try_into().ok()?) as usize;
    let (mut rgb, mut gray) = (None, None);
    for i in 0..n.min(256) {
        let e = d.get(132 + 12 * i..144 + 12 * i)?;
        let off = u32::from_be_bytes(e[4..8].try_into().ok()?) as usize;
        let len = u32::from_be_bytes(e[8..12].try_into().ok()?) as usize;
        // Profile values: 'prof' + 4 reserved bytes, then the ICC profile itself.
        let v = d.get(off..off.checked_add(len)?)?;
        let icc = v.get(8..).filter(|b| b.len() > 128).map(<[u8]>::to_vec);
        match &e[0..4] {
            b"wRGB" => rgb = icc,
            b"wGry" => gray = icc,
            _ => {}
        }
    }
    Some((rgb, gray))
}

fn default_csf() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let prefs = Path::new(&home).join("Library/Preferences");
    let mut best: Option<(String, PathBuf)> = None;
    for e in std::fs::read_dir(&prefs).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("Adobe Photoshop 20") && name.ends_with(" Settings") {
            let p = e.path().join("Color Settingscsf");
            if p.exists() && best.as_ref().is_none_or(|(b, _)| name > *b) {
                best = Some((name, p));
            }
        }
    }
    best.map(|b| b.1)
}

/// Lab (D50, relative colorimetric) of RGB/gray pixels already over white, through `profile`.
fn to_lab(px: &[[f32; 3]], profile_bytes: Option<&[u8]>, gray: bool, linear: bool) -> Result<Vec<[f32; 3]>, String> {
    let make_src = || -> Result<Profile, String> {
        let base = match profile_bytes {
            Some(b) => Profile::new_icc(b).map_err(|e| format!("embedded profile: {e}"))?,
            None if gray => Profile::new_gray(&lcms2::CIExyY { x: 0.3457, y: 0.3585, Y: 1.0 }, &lcms2::ToneCurve::new(2.2)).map_err(|e| e.to_string())?,
            None => Profile::new_srgb(),
        };
        if !linear || gray {
            return Ok(base);
        }
        // 32-bit documents hold linear values in the working space's primaries.
        let xyz = |sig| match base.read_tag(sig) {
            lcms2::Tag::CIEXYZ(v) => Some(*v),
            _ => None,
        };
        let (Some(r), Some(g), Some(b)) =
            (xyz(lcms2::TagSignature::RedColorantTag), xyz(lcms2::TagSignature::GreenColorantTag), xyz(lcms2::TagSignature::BlueColorantTag))
        else {
            return Ok(base);
        };
        // Colorants are D50-adapted; build a linear D50 matrix profile from them.
        let to_xyy = |c: lcms2::CIEXYZ| {
            let s = c.X + c.Y + c.Z;
            lcms2::CIExyY { x: c.X / s, y: c.Y / s, Y: c.Y }
        };
        let prim = lcms2::CIExyYTRIPLE { Red: to_xyy(r), Green: to_xyy(g), Blue: to_xyy(b) };
        let lin = lcms2::ToneCurve::new(1.0);
        Profile::new_rgb(&lcms2::CIExyY { x: 0.3457, y: 0.3585, Y: 1.0 }, &prim, &[&lin, &lin, &lin]).map_err(|e| e.to_string())
    };
    use rayon::prelude::*;
    let out: Result<Vec<Vec<[f32; 3]>>, String> = px
        .par_chunks(1 << 18)
        .map(|chunk| {
            let src = make_src()?;
            let lab = Profile::new_lab4_context(lcms2::GlobalContext::new(), &lcms2::CIExyY { x: 0.3457, y: 0.3585, Y: 1.0 }).map_err(|e| e.to_string())?;
            let mut out = vec![[0f32; 3]; chunk.len()];
            if gray {
                let g: Vec<f32> = chunk.iter().map(|p| p[0]).collect();
                let t: Transform<f32, [f32; 3]> =
                    Transform::new(&src, PixelFormat::GRAY_FLT, &lab, PixelFormat::Lab_FLT, Intent::RelativeColorimetric).map_err(|e| e.to_string())?;
                t.transform_pixels(&g, &mut out);
            } else {
                let t: Transform<[f32; 3], [f32; 3]> =
                    Transform::new(&src, PixelFormat::RGB_FLT, &lab, PixelFormat::Lab_FLT, Intent::RelativeColorimetric).map_err(|e| e.to_string())?;
                t.transform_pixels(chunk, &mut out);
            }
            Ok(out)
        })
        .collect();
    Ok(out?.into_iter().flatten().collect())
}

/// Feature tags of one layer.
fn layer_features(l: &Layer) -> Vec<String> {
    let mut f = Vec::new();
    match &l.content {
        LayerContent::Adjustment(a) => {
            let s = format!("{a:?}");
            f.push(format!("adj:{}", s.split([' ', '{', '(']).next().unwrap_or("?")));
        }
        LayerContent::Fill(x) => {
            let s = format!("{x:?}");
            f.push(format!("fill:{}", s.split([' ', '{', '(']).next().unwrap_or("?")));
        }
        LayerContent::Text(_) => f.push("text".into()),
        LayerContent::Shape(_) => f.push("shape".into()),
        LayerContent::Smart(_) => f.push("smart object".into()),
        LayerContent::Raster(_) => f.push("pixel".into()),
        LayerContent::Group(_) => {}
    }
    let blend = format!("{:?}", l.blend);
    if blend != "Normal" && blend != "PassThrough" {
        f.push(format!("blend:{blend}"));
    }
    if l.effects.enabled {
        for e in &l.effects.items {
            if e.enabled() {
                let s = format!("{e:?}");
                f.push(format!("style:{}", s.split([' ', '{', '(']).next().unwrap_or("?")));
            }
        }
    }
    if l.clipped {
        f.push("clipped".into());
    }
    if l.mask.as_ref().is_some_and(|m| m.enabled) {
        f.push("layer mask".into());
    }
    if l.vector_mask.as_ref().is_some_and(|m| m.enabled) {
        f.push("vector mask".into());
    }
    if l.opacity < 0.999 || l.fill_opacity < 0.999 {
        f.push("opacity<100".into());
    }
    f
}

/// The canvas area a layer affects (its content, grown for effects; adjustments: mask area or canvas).
fn layer_region(l: &Layer, canvas: photocraft_doc::Rect) -> photocraft_doc::Rect {
    let mut r = canvas;
    if let Some(s) = l.surface() {
        r = s.content_bounds();
    } else if let Some(m) = l.mask.as_ref().filter(|m| m.enabled && m.surface.default_pixel().first().is_some_and(|v| *v <= 0.0)) {
        r = m.surface.content_bounds();
    }
    if let Some(vm) = l.vector_mask.as_ref().filter(|m| m.enabled && !m.path.inverted)
        && let Some((x0, y0, x1, y1)) = vm.path.control_bounds()
    {
        r = photocraft_doc::Rect::new(x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32, y1.ceil() as i32);
    }
    if l.effects.enabled && l.effects.items.iter().any(|e| e.enabled()) {
        r = photocraft_doc::Rect::new(r.x0 - 40, r.y0 - 40, r.x1 + 40, r.y1 + 40);
    }
    r.intersect(&canvas)
}

fn visible_layers(layers: &[Layer], parent_feats: &[String], out: &mut Vec<(Vec<String>, photocraft_doc::Rect)>, canvas: photocraft_doc::Rect, depth: usize) {
    if depth > 64 {
        return;
    }
    for l in layers {
        if !l.visible {
            continue;
        }
        let mut feats = layer_features(l);
        if let LayerContent::Group(g) = &l.content {
            // Group blend/style tags apply to everything inside.
            let mut inherited: Vec<String> = parent_feats.to_vec();
            inherited.extend(feats.iter().filter(|f| f.starts_with("blend:") || f.starts_with("style:")).cloned());
            visible_layers(&g.children, &inherited, out, canvas, depth + 1);
            continue;
        }
        feats.extend(parent_feats.iter().cloned());
        out.push((feats, layer_region(l, canvas)));
    }
}

struct FileResult {
    json: Value,
    feature_hists: BTreeMap<String, Hist>,
}

fn measure(path: &Path, prof: &Working, max_mp: f64, out_dir: Option<&Path>, panel_w: u32) -> Result<FileResult, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let file = photocraft_psd::PsdFile::from_bytes(&bytes).map_err(|e| format!("psd: {e}"))?;
    let (w, h) = (file.header.width as usize, file.header.height as usize);
    let mp = (w * h) as f64 / 1e6;
    if mp > max_mp {
        return Err(format!("skipped: {mp:.0} MP > --max-mp"));
    }
    if file.has_real_merged_data() == Some(false) {
        return Err("no real merged composite (Maximize Compatibility was off)".into());
    }
    let t0 = std::time::Instant::now();
    let imp = photocraft_io::import(&name, &bytes).map_err(|e| format!("import: {e}"))?;
    let doc: &Document = &imp.document;
    let ours = photocraft_compose::flatten(doc).px;
    let t_render = t0.elapsed().as_secs_f64();
    // Photoshop's composite at full precision (16/32-bit files keep their depth).
    let merged = if doc.depth == SampleType::U8 {
        photocraft_io::merged_composite(&file).map_err(|e| format!("composite: {e}"))?
    } else {
        let (md, _) = photocraft_io::psd_to_document(&photocraft_psd::PsdFile { layer_info: None, ..file.clone() });
        let l = md.layers.first().ok_or("no merged image")?;
        let s = l.surface().ok_or("no merged image")?;
        photocraft_compose::surface_to_buffer(s, md.bounds()).px
    };
    drop(bytes);
    if merged.len() != ours.len() {
        return Err(format!("size mismatch: ours {} px, Photoshop {} px", ours.len(), merged.len()));
    }
    let linear = doc.depth == SampleType::F32;
    let over = |p: &[f32; 4]| -> [f32; 3] {
        let a = p[3].clamp(0.0, 1.0);
        [p[0] * a + 1.0 - a, p[1] * a + 1.0 - a, p[2] * a + 1.0 - a]
    };
    let gray = doc.mode == ColorMode::Grayscale;
    let approx = !matches!(doc.mode, ColorMode::Rgb | ColorMode::Grayscale);
    let embedded = doc.icc_profile.as_ref().map(|b| b.as_slice().to_vec());
    let profile_desc = embedded.as_deref().and_then(|b| Profile::new_icc(b).ok()).and_then(|p| p.info(lcms2::InfoType::Description, lcms2::Locale::none()));
    let use_profile: Option<Vec<u8>> = if approx { None } else { embedded.clone().or_else(|| if gray { prof.gray.clone() } else { prof.rgb.clone() }) };
    let a: Vec<[f32; 3]> = ours.iter().map(over).collect();
    let b: Vec<[f32; 3]> = merged.iter().map(over).collect();
    let la = to_lab(&a, use_profile.as_deref(), gray, linear)?;
    let lb = to_lab(&b, use_profile.as_deref(), gray, linear)?;
    use rayon::prelude::*;
    let de: Vec<f32> = la.par_iter().zip(lb.par_iter()).map(|(x, y)| de2000(*x, *y)).collect();
    let mut total = Hist::new();
    for d in &de {
        total.add(*d);
    }
    // Per feature: pixels covered by the layers that have it.
    let canvas = doc.bounds();
    let mut regions = Vec::new();
    visible_layers(&doc.layers, &[], &mut regions, canvas, 0);
    let mut by_feat: BTreeMap<String, Vec<photocraft_doc::Rect>> = BTreeMap::new();
    for (feats, r) in &regions {
        for f in feats {
            by_feat.entry(f.clone()).or_default().push(*r);
        }
    }
    let mode_tag = format!(
        "doc:{:?} {}",
        doc.mode,
        match doc.depth {
            SampleType::U8 => "8-bit",
            SampleType::U16 => "16-bit",
            SampleType::F32 => "32-bit",
        }
    );
    let prof_tag = format!("profile:{}", profile_desc.clone().unwrap_or_else(|| "none (working space)".into()));
    let mut feature_hists = BTreeMap::new();
    feature_hists.insert(mode_tag.clone(), total.clone());
    feature_hists.insert(prof_tag.clone(), total.clone());
    for (f, rects) in &by_feat {
        let mut mask = vec![false; w * h];
        for r in rects {
            for y in r.y0.max(0)..r.y1.min(h as i32) {
                let row = y as usize * w;
                for x in r.x0.max(0)..r.x1.min(w as i32) {
                    mask[row + x as usize] = true;
                }
            }
        }
        let mut hst = Hist::new();
        for (d, m) in de.iter().zip(&mask) {
            if *m {
                hst.add(*d);
            }
        }
        if hst.n > 0 {
            feature_hists.insert(f.clone(), hst);
        }
    }
    if let Some(dir) = out_dir {
        // Parent folder + name: corpora repeat file names across folders (rgb8/levels.psd, …).
        let parent = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let stem: String = format!("{parent}__{name}").chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
        let fd = dir.join(stem);
        std::fs::create_dir_all(&fd).map_err(|e| e.to_string())?;
        let pw = panel_w.min(w as u32).max(1);
        let ph = ((h as f64 * f64::from(pw) / w as f64).round() as u32).max(1);
        let sample = |img: &dyn Fn(usize) -> [u8; 3]| {
            let mut buf = image::RgbImage::new(pw, ph);
            for (x, y, p) in buf.enumerate_pixels_mut() {
                let sx = ((f64::from(x) + 0.5) * w as f64 / f64::from(pw)) as usize;
                let sy = ((f64::from(y) + 0.5) * h as f64 / f64::from(ph)) as usize;
                *p = image::Rgb(img(sy.min(h - 1) * w + sx.min(w - 1)));
            }
            buf
        };
        // Panels show document values as sRGB (a preview, not a colour-managed render).
        let q = |v: f32| -> u8 {
            let v = if linear { if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 } } else { v };
            (v.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        let _ = sample(&|i| [q(b[i][0]), q(b[i][1]), q(b[i][2])]).save(fd.join("ps.png"));
        let _ = sample(&|i| [q(a[i][0]), q(a[i][1]), q(a[i][2])]).save(fd.join("ours.png"));
        // ΔE heat map: black 0, red 5, yellow 10+ (max over each panel cell would hide less, but sampling is enough to locate).
        let heat = |i: usize| {
            let d = de[i];
            let t = (d / 10.0).clamp(0.0, 1.0);
            if t < 0.5 { [(t * 2.0 * 255.0) as u8, 0, 0] } else { [255, ((t - 0.5) * 2.0 * 255.0) as u8, 0] }
        };
        let _ = sample(&heat).save(fd.join("de.png"));
    }
    let t_all = t0.elapsed().as_secs_f64();
    let mut feats: Vec<&String> = by_feat.keys().collect();
    feats.sort();
    let json = json!({
        "file": name, "path": path.display().to_string(), "width": w, "height": h, "mode": mode_tag, "profile": prof_tag,
        "approx": approx, "layers": doc.layer_count(), "warnings": imp.warnings.iter().take(4).collect::<Vec<_>>(),
        "features": feats, "total": total.json(), "render_s": r3(t_render), "time_s": r3(t_all),
    });
    Ok(FileResult { json, feature_hists })
}

struct Working {
    rgb: Option<Vec<u8>>,
    gray: Option<Vec<u8>>,
}

fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for c in v {
            collect(&c, out);
        }
    } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("psb")) {
        out.push(p.to_path_buf());
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut out_dir, mut baseline, mut csf, mut max_mp, mut panel_w) = (None::<PathBuf>, None::<PathBuf>, None::<PathBuf>, 60.0f64, 900u32);
    let mut inputs = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out_dir = args.next().map(PathBuf::from),
            "--baseline" => baseline = args.next().map(PathBuf::from),
            "--csf" => csf = args.next().map(PathBuf::from),
            "--max-mp" => max_mp = args.next().and_then(|v| v.parse().ok()).unwrap_or(max_mp),
            "--panel-width" => panel_w = args.next().and_then(|v| v.parse().ok()).unwrap_or(panel_w),
            "-h" | "--help" => {
                println!("ps-compare [--out DIR] [--baseline report.json] [--csf file] [--max-mp N] [--panel-width N] <psd|dir>...");
                return;
            }
            _ => inputs.push(PathBuf::from(a)),
        }
    }
    let csf = csf.or_else(default_csf);
    let (rgb, gray) = csf.as_deref().and_then(csf_working).unwrap_or((None, None));
    eprintln!(
        "working spaces from {}: RGB {}, Gray {}",
        csf.as_deref().map(|p| p.display().to_string()).unwrap_or_else(|| "(none)".into()),
        rgb.as_deref()
            .and_then(|b| Profile::new_icc(b).ok())
            .and_then(|p| p.info(lcms2::InfoType::Description, lcms2::Locale::none()))
            .unwrap_or_else(|| "sRGB (built in)".into()),
        gray.as_deref()
            .and_then(|b| Profile::new_icc(b).ok())
            .and_then(|p| p.info(lcms2::InfoType::Description, lcms2::Locale::none()))
            .unwrap_or_else(|| "gamma 2.2 (built in)".into()),
    );
    let working = Working { rgb, gray };
    let mut files = Vec::new();
    for i in &inputs {
        collect(i, &mut files);
    }
    let mut rows = Vec::new();
    let mut feat_all: BTreeMap<String, (Vec<(String, f64, f64)>, Hist)> = BTreeMap::new();
    for f in &files {
        let r = std::panic::catch_unwind(|| measure(f, &working, max_mp, out_dir.as_deref(), panel_w));
        match r {
            Ok(Ok(res)) => {
                let t = &res.json["total"];
                println!(
                    "{:<62} {:>7.3} {:>7.3} {:>7.2} {:>6.2}%  {}",
                    res.json["file"].as_str().unwrap_or_default(),
                    t["mean"].as_f64().unwrap_or(0.0),
                    t["p95"].as_f64().unwrap_or(0.0),
                    t["max"].as_f64().unwrap_or(0.0),
                    t["over2"].as_f64().unwrap_or(0.0),
                    res.json["mode"].as_str().unwrap_or_default()
                );
                let fname = res.json["file"].as_str().unwrap_or_default().to_string();
                for (k, h) in &res.feature_hists {
                    let e = feat_all.entry(k.clone()).or_insert_with(|| (Vec::new(), Hist::new()));
                    e.0.push((fname.clone(), h.mean(), h.pct(0.95)));
                    // Pool at most 2 MP per file so one large file does not decide a feature.
                    let scale = (2e6 / h.n.max(1) as f64).min(1.0);
                    for (i, b) in h.bins.iter().enumerate() {
                        let n = (*b as f64 * scale).round() as u64;
                        e.1.bins[i] += n;
                        e.1.n += n;
                        e.1.sum += n as f64 * (i as f64 + 0.5) * f64::from(HIST_STEP);
                    }
                    e.1.max = e.1.max.max(h.max);
                }
                rows.push(res.json);
            }
            Ok(Err(e)) => {
                println!("{:<62} {e}", f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
                rows.push(json!({"file": f.file_name().map(|n| n.to_string_lossy().to_string()), "path": f.display().to_string(), "error": e}));
            }
            Err(_) => {
                println!("{:<62} PANIC", f.display());
                rows.push(json!({"file": f.file_name().map(|n| n.to_string_lossy().to_string()), "path": f.display().to_string(), "error": "panic"}));
            }
        }
    }
    let features: Vec<Value> = feat_all
        .iter()
        .map(|(k, (files, h))| {
            let n = files.len() as f64;
            let worst = files.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|w| w.0.clone()).unwrap_or_default();
            json!({"feature": k, "files": files.len(), "mean_of_files": r3(files.iter().map(|f| f.1).sum::<f64>() / n.max(1.0)),
                   "p95_of_files": r3(files.iter().map(|f| f.2).sum::<f64>() / n.max(1.0)), "pooled": h.json(), "worst": worst})
        })
        .collect();
    println!("\n{:<44} {:>5} {:>8} {:>8} {:>8}  worst file", "feature", "files", "mean", "p95", "pooled95");
    for f in &features {
        println!(
            "{:<44} {:>5} {:>8.3} {:>8.3} {:>8.3}  {}",
            f["feature"].as_str().unwrap_or_default(),
            f["files"],
            f["mean_of_files"].as_f64().unwrap_or(0.0),
            f["p95_of_files"].as_f64().unwrap_or(0.0),
            f["pooled"]["p95"].as_f64().unwrap_or(0.0),
            f["worst"].as_str().unwrap_or_default()
        );
    }
    let report = json!({"files": rows, "features": features});
    if let Some(base) = baseline.as_deref().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        println!("\nbefore → after (mean ΔE2000 / p95)");
        // Matched by path (corpora repeat file names across folders).
        let bf: BTreeMap<String, &Value> = base["files"].as_array().into_iter().flatten().filter_map(|r| Some((r["path"].as_str()?.to_string(), r))).collect();
        for r in report["files"].as_array().into_iter().flatten() {
            let Some(path) = r["path"].as_str() else { continue };
            let name = std::path::Path::new(path).iter().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect::<PathBuf>().display().to_string();
            if let Some(b) = bf.get(path) {
                println!(
                    "{:<62} {:>7.3} → {:>7.3}   {:>7.3} → {:>7.3}",
                    name,
                    b["total"]["mean"].as_f64().unwrap_or(f64::NAN),
                    r["total"]["mean"].as_f64().unwrap_or(f64::NAN),
                    b["total"]["p95"].as_f64().unwrap_or(f64::NAN),
                    r["total"]["p95"].as_f64().unwrap_or(f64::NAN)
                );
            }
        }
        let bfe: BTreeMap<String, &Value> =
            base["features"].as_array().into_iter().flatten().filter_map(|r| Some((r["feature"].as_str()?.to_string(), r))).collect();
        for f in report["features"].as_array().into_iter().flatten() {
            let Some(name) = f["feature"].as_str() else { continue };
            if let Some(b) = bfe.get(name) {
                println!(
                    "{:<44} {:>7.3} → {:>7.3}   {:>7.3} → {:>7.3}",
                    name,
                    b["mean_of_files"].as_f64().unwrap_or(f64::NAN),
                    f["mean_of_files"].as_f64().unwrap_or(f64::NAN),
                    b["p95_of_files"].as_f64().unwrap_or(f64::NAN),
                    f["p95_of_files"].as_f64().unwrap_or(f64::NAN)
                );
            }
        }
    }
    if let Some(dir) = out_dir {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("report.json"), serde_json::to_vec_pretty(&report).unwrap_or_default());
    }
}
