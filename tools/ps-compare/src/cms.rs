//! cms-compare: PhotoCraft's colour engine (photocraft-cms) against Little CMS, profile pair by
//! profile pair, on a grid of colours. The difference is reported in CIEDE2000, measured in the
//! destination space (both results read through the destination profile with Little CMS).
//!
//! ```sh
//! cargo run --release -p photocraft-ps-compare --bin cms-compare -- SRC.icc DST.icc [INTENT] [bpc|nobpc] ...
//!   INTENT: perceptual | relative | saturation | absolute (default relative); repeat the 4-tuple for more pairs
//! cms-compare --dump-builtin coated-cmyk out.icc      # write a PhotoCraft built-in profile
//! ```
//! Photoshop's own engine (Adobe ACE) is closed; Little CMS is the reference here because it is the
//! open CMM that matches ACE most closely (Adobe's black point compensation algorithm, published
//! by Adobe, is what both implement).
#![allow(clippy::too_many_arguments, clippy::type_complexity)] // a measurement tool, not product code

use lcms2::{Intent, PixelFormat, Profile, Transform};
use photocraft_cms::Intent as PcIntent;

fn grid(channels: usize) -> Vec<Vec<f32>> {
    let steps: usize = match channels {
        1 => 256,
        3 => 17,
        _ => 9,
    };
    let mut out = Vec::new();
    let n = steps.pow(channels as u32);
    for i in 0..n {
        let mut v = Vec::with_capacity(channels);
        let mut k = i;
        for _ in 0..channels {
            v.push((k % steps) as f32 / (steps - 1) as f32);
            k /= steps;
        }
        out.push(v);
    }
    out
}

fn fmt(ch: usize) -> PixelFormat {
    match ch {
        1 => PixelFormat::GRAY_FLT,
        4 => PixelFormat::CMYK_FLT,
        _ => PixelFormat::RGB_FLT,
    }
}

/// lcms float CMYK is 0..100; everything else 0..1.
fn scale(ch: usize) -> f32 {
    if ch == 4 { 100.0 } else { 1.0 }
}

fn lcms_intent(s: &str) -> (Intent, PcIntent) {
    match s {
        "perceptual" => (Intent::Perceptual, PcIntent::Perceptual),
        "saturation" => (Intent::Saturation, PcIntent::Saturation),
        "absolute" => (Intent::AbsoluteColorimetric, PcIntent::AbsoluteColorimetric),
        _ => (Intent::RelativeColorimetric, PcIntent::RelativeColorimetric),
    }
}

/// One lcms float transform over flat samples (`A` in, `B` out channels per pixel).
fn xf_n<const A: usize, const B: usize>(
    src: &Profile,
    sf: PixelFormat,
    dst: &Profile,
    df: PixelFormat,
    intent: Intent,
    flags: lcms2::Flags,
    input: &[f32],
) -> Result<Vec<f32>, String> {
    let t: Transform<[f32; A], [f32; B]> = Transform::new_flags(src, sf, dst, df, intent, flags).map_err(|e| e.to_string())?;
    let inp: Vec<[f32; A]> = input.chunks(A).map(|c| std::array::from_fn(|i| c.get(i).copied().unwrap_or(0.0))).collect();
    let mut out = vec![[0f32; B]; inp.len()];
    t.transform_pixels(&inp, &mut out);
    Ok(out.into_iter().flatten().collect())
}

fn xf(src: &Profile, sc: usize, dst: &Profile, dc: usize, df: PixelFormat, intent: Intent, flags: lcms2::Flags, input: &[f32]) -> Result<Vec<f32>, String> {
    let sf = fmt(sc);
    match (sc, dc) {
        (1, 1) => xf_n::<1, 1>(src, sf, dst, df, intent, flags, input),
        (1, 3) => xf_n::<1, 3>(src, sf, dst, df, intent, flags, input),
        (1, 4) => xf_n::<1, 4>(src, sf, dst, df, intent, flags, input),
        (3, 1) => xf_n::<3, 1>(src, sf, dst, df, intent, flags, input),
        (3, 3) => xf_n::<3, 3>(src, sf, dst, df, intent, flags, input),
        (3, 4) => xf_n::<3, 4>(src, sf, dst, df, intent, flags, input),
        (4, 1) => xf_n::<4, 1>(src, sf, dst, df, intent, flags, input),
        (4, 3) => xf_n::<4, 3>(src, sf, dst, df, intent, flags, input),
        (4, 4) => xf_n::<4, 4>(src, sf, dst, df, intent, flags, input),
        _ => Err(format!("unsupported channel counts {sc} → {dc}")),
    }
}

fn to_lab(p: &Profile, ch: usize, px: &[Vec<f32>]) -> Result<Vec<[f32; 3]>, String> {
    let lab = Profile::new_lab4_context(lcms2::GlobalContext::new(), &lcms2::CIExyY { x: 0.3457, y: 0.3585, Y: 1.0 }).map_err(|e| e.to_string())?;
    let flat: Vec<f32> = px.iter().flat_map(|v| v.iter().map(|x| x * scale(ch))).collect();
    let out = xf(p, ch, &lab, 3, PixelFormat::Lab_FLT, Intent::RelativeColorimetric, lcms2::Flags::default(), &flat)?;
    Ok(out.chunks(3).map(|c| [c[0], c[1], c[2]]).collect())
}

fn run(src_path: &str, dst_path: &str, intent: &str, bpc: bool) -> Result<String, String> {
    let sb = std::fs::read(src_path).map_err(|e| format!("{src_path}: {e}"))?;
    let db = std::fs::read(dst_path).map_err(|e| format!("{dst_path}: {e}"))?;
    let (li, pi) = lcms_intent(intent);
    let ls = Profile::new_icc(&sb).map_err(|e| e.to_string())?;
    let ld = Profile::new_icc(&db).map_err(|e| e.to_string())?;
    let ps = photocraft_cms::Profile::parse(&sb).map_err(|e| format!("photocraft parse {src_path}: {e}"))?;
    let pd = photocraft_cms::Profile::parse(&db).map_err(|e| format!("photocraft parse {dst_path}: {e}"))?;
    let (sc, dc) = (ps.channels(), pd.channels());
    let samples = grid(sc);
    // Little CMS.
    let flags = if bpc { lcms2::Flags::BLACKPOINT_COMPENSATION } else { lcms2::Flags::default() };
    let flat: Vec<f32> = samples.iter().flat_map(|v| v.iter().map(|x| x * scale(sc))).collect();
    let lout = xf(&ls, sc, &ld, dc, fmt(dc), li, flags, &flat)?;
    let lres: Vec<Vec<f32>> = lout.chunks(dc).map(|c| c.iter().map(|x| x / scale(dc)).collect()).collect();
    // PhotoCraft.
    let pt = photocraft_cms::Transform::new(&ps, &pd, pi, bpc).map_err(|e| format!("photocraft transform: {e}"))?;
    let mut pres = Vec::with_capacity(samples.len());
    for s in &samples {
        let mut o = vec![0f32; dc];
        pt.eval(s, &mut o);
        pres.push(o);
    }
    let a = to_lab(&ld, dc, &lres)?;
    let b = to_lab(&ld, dc, &pres)?;
    let mut d: Vec<f32> = a.iter().zip(&b).map(|(x, y)| photocraft_ps_compare::de2000(*x, *y)).collect();
    let mean = d.iter().map(|v| f64::from(*v)).sum::<f64>() / d.len().max(1) as f64;
    d.sort_by(f32::total_cmp);
    let p95 = d.get((d.len() as f64 * 0.95) as usize).copied().unwrap_or(0.0);
    let max = d.last().copied().unwrap_or(0.0);
    let worst = a
        .iter()
        .zip(&b)
        .enumerate()
        .max_by(|x, y| photocraft_ps_compare::de2000(*x.1.0, *x.1.1).total_cmp(&photocraft_ps_compare::de2000(*y.1.0, *y.1.1)))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let name = |p: &str| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(format!(
        "{:<34} → {:<34} {:<10} {:<5} mean {:>6.3}  p95 {:>6.3}  max {:>6.2}  (worst in {:?}: lcms {:?} pc {:?})",
        name(src_path),
        name(dst_path),
        intent,
        if bpc { "bpc" } else { "-" },
        mean,
        p95,
        max,
        samples[worst],
        lres[worst],
        pres[worst]
    ))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // --dump-builtin ID OUT.icc: write one of PhotoCraft's built-in profiles (to compare it).
    if args.first().map(String::as_str) == Some("--dump-builtin") {
        let (Some(id), Some(out)) = (args.get(1), args.get(2)) else {
            eprintln!("--dump-builtin ID OUT.icc");
            return;
        };
        match photocraft_cms::Builtin::from_id(id) {
            Some(b) => {
                if let Err(e) = std::fs::write(out, b.profile().to_bytes().as_slice()) {
                    eprintln!("{out}: {e}");
                }
            }
            None => eprintln!("unknown built-in `{id}`"),
        }
        return;
    }
    let mut i = 0;
    while i + 1 < args.len() {
        let (s, d) = (&args[i], &args[i + 1]);
        let intent = args.get(i + 2).map(String::as_str).unwrap_or("relative");
        let bpc = args.get(i + 3).map(String::as_str) != Some("nobpc");
        match run(s, d, intent, bpc) {
            Ok(line) => println!("{line}"),
            Err(e) => println!("{s} → {d}: {e}"),
        }
        i += 4;
    }
}
