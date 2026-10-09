# Progress — Bryan's PhotoCraft (his copy of storytold/photocraft, ~/crafts/photocraft)

Bryan wants PhotoCraft to work like his Photoshop 2026: his shortcuts, actions, brushes, patterns, swatches, guides,
a shortcut editor in settings, and devices (Logi MX Creative Console) that drive it by keystroke.
Rules are in AGENTS.md (never crash, everything is a command, *Fork rules*). Since 2026-10-09 this is his own fork: Adobe SDKs / C deps
and the Adobe colour data on his Mac are allowed at runtime, nothing Adobe committed. This board is for his changes.
Commit on `main`; `origin` = github.com/bryanhempstead/photocraft (his fork), `upstream` = storytold.

## Open
- [ ] (colour agent, 2026-10-09) Next colour work, by measured size (tools/ps-compare on corpus/photoshop): Color Balance (rgb8 mean ΔE 6.0: Photoshop's is an exact per-channel LUT — proven, output channel depends only on its input channel — but the slider→LUT formula needs more samples), Vibrance (2.9), Brightness/Contrast legacy (1.5), 32-bit adjustments (levels/exposure/hue-sat 6–8), CMYK/Lab adjustments (8+). **Bryan could help**: save 3–4 small PSDs with Color Balance / Vibrance at different slider settings (Maximize Compatibility on) into the corpus — each one pins the formula.
- [ ] (colour agent) Camera Raw in PhotoCraft (crates/raw/develop.rs) uses the DNG ColorMatrix → linear ProPhoto, no Adobe Standard DCP / ACR tone curve: once LightCraft's dng-sdk-sys + DCP path lands, port it here (fork rules allow).
- [x] (colour agent, 2026-10-09) Photoshop colour match round 1 — 95f75b0 (fork rules), 285b43e (tools/ps-compare + cms-compare), 17f5123 (knockout, colorize, B&W tint), 0fb6117 (Color Settings from Photoshop), 945c273 (corpus floors psd-tools 237, photoshop 135)
- [ ] Run the real migration into his settings once he says so (quit PhotoCraft first):
      `target/release/photocraft-cli migrate-photoshop` (or `cargo run --release -p photocraft-cli -- migrate-photoshop`) — defaults: newest PS settings folder → ~/Library/Application Support/Photocraft
- [ ] Actions: an ActionDescriptor interpreter for select/set/make/move/delete by reference (most of his GLASSMORPH/CUTOUT/NOISE steps); today 0 of 24 actions are fully runnable
- [ ] Logi MX Creative Console: build a PhotoCraft profile (bundle id ai.storyteller.photocraft) from tools/devices/README.md
- [ ] Missing tools his shortcuts name: Selection Brush (L), Remove (J), Curvature Pen (P), Star (U), Perspective Crop, Frame, Color Sampler, Red Eye…; layer styles (.asl) import
- [x] (ps-migrate agent, 2026-10-08) Photoshop migration: shortcuts (.kys) + keymap.json + shrt. editor, actions (.atn), brushes, patterns, swatches, guides; device commands + keys; app.place bridge — e0b5bf8, 287d3b6, 48f961c

## Notes for the next agent
- (colour agent, 2026-10-09) **Measuring colour vs Photoshop**: `cargo build --release -p photocraft-ps-compare`, then
  `target/release/ps-compare --out DIR [--baseline OLD/report.json] <psd|dir>…` (layers rendered by PhotoCraft vs the merged
  composite Photoshop stored in the file → ΔE2000 per file + per feature, PS|ours|ΔE panels) and
  `target/release/cms-compare SRC.icc DST.icc relative bpc …` (photocraft-cms vs Little CMS). Corpora: `corpus/photoshop`
  and `corpus/psd-tools` (gitignored; fetch at the pins). Bryan's 41-file sample (copies) sat in the colour agent's scratchpad
  `ps-match/files`: 40/41 already matched (mean ≤ 0.5); NastyCopy (knockout + colorize) 3.30 → 0.31.
- (colour agent) photocraft-cms matches Little CMS to ≤ 0.24 mean ΔE on every pair tried (sRGB/AdobeRGB/ProPhoto/P3/Studio
  Display/SWOP/FOGRA39/Dot Gain 20%, rel+BPC and perceptual) — no need to swap CMMs. The real gap was Color Settings:
  PhotoCraft's working CMYK/Gray were synthetic "coated-cmyk"/sGray vs his Photoshop's U.S. Web Coated (SWOP) v2 / Dot Gain 20%
  (Image › Mode › CMYK look ΔE 5.0 mean / 12.9 p95; untagged gray 4.7). `crates/engine/src/photoshop_color.rs` reads
  Photoshop's `Color Settingscsf` at runtime: specs `photoshop-rgb|cmyk|gray`, command `edit.colorSettings.importPhotoshop`,
  and the desktop app adopts them at launch when PhotoCraft's Color Settings are still the defaults (prefs_ui::load).
- Photoshop migration (2026-10-08): engine `crates/engine/src/photoshop_cmds*` (file.migrateFromPhotoshop, edit.keyboardShortcuts.importKys,
  actions.importAtn), presets/swatches.rs (swatches.*, view.guideLayout.presets), psd atn/aco/phry. CLI `migrate-photoshop`.
  Dry run on his real data (scratch config): 8 keys changed + 2 unbound + 5 extra keys, 24 actions, 105 brushes / 17 groups
  (28 stock skipped), 23 patterns (10 stock skipped), 2 swatches, 6 guide layouts. Device key table: tools/devices/README.md.
- file.migrateFromPhotoshop is refused over the control channel (ambient paths) by design; use the CLI or File menu.
