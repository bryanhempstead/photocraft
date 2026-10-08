# Progress — Bryan's PhotoCraft (his copy of storytold/photocraft, ~/crafts/photocraft)

Bryan wants PhotoCraft to work like his Photoshop 2026: his shortcuts, actions, brushes, patterns, swatches, guides,
a shortcut editor in settings, and devices (Logi MX Creative Console) that drive it by keystroke.
Upstream rules are in AGENTS.md (never crash, pure Rust, everything is a command). This board is for his changes.
Commit on `main`; `origin` = github.com/bryanhempstead/photocraft (his fork), `upstream` = storytold.

## Open
- [ ] Run the real migration into his settings once he says so (quit PhotoCraft first):
      `target/release/photocraft-cli migrate-photoshop` (or `cargo run --release -p photocraft-cli -- migrate-photoshop`) — defaults: newest PS settings folder → ~/Library/Application Support/Photocraft
- [ ] Actions: an ActionDescriptor interpreter for select/set/make/move/delete by reference (most of his GLASSMORPH/CUTOUT/NOISE steps); today 0 of 24 actions are fully runnable
- [ ] Logi MX Creative Console: build a PhotoCraft profile (bundle id ai.storyteller.photocraft) from tools/devices/README.md
- [ ] Missing tools his shortcuts name: Selection Brush (L), Remove (J), Curvature Pen (P), Star (U), Perspective Crop, Frame, Color Sampler, Red Eye…; layer styles (.asl) import
- [x] (ps-migrate agent, 2026-10-08) Photoshop migration: shortcuts (.kys) + keymap.json + shrt. editor, actions (.atn), brushes, patterns, swatches, guides; device commands + keys; app.place bridge — e0b5bf8, 287d3b6, 48f961c

## Notes for the next agent
- Photoshop migration (2026-10-08): engine `crates/engine/src/photoshop_cmds*` (file.migrateFromPhotoshop, edit.keyboardShortcuts.importKys,
  actions.importAtn), presets/swatches.rs (swatches.*, view.guideLayout.presets), psd atn/aco/phry. CLI `migrate-photoshop`.
  Dry run on his real data (scratch config): 8 keys changed + 2 unbound + 5 extra keys, 24 actions, 105 brushes / 17 groups
  (28 stock skipped), 23 patterns (10 stock skipped), 2 swatches, 6 guide layouts. Device key table: tools/devices/README.md.
- file.migrateFromPhotoshop is refused over the control channel (ambient paths) by design; use the CLI or File menu.
