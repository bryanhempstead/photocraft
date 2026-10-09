# PhotoCraft (Bryan's fork) — session rules

- Read `AGENTS.md` first (map, golden rules, **Fork rules**), then `.agent/PROGRESS.md` (the shared work board). End: tick
  what you did with the commit hash and leave the next step on the board.
- This is Bryan's own fork (since 2026-10-09): not clean-room, not pure-Rust. Adobe SDKs / C or C++ deps are allowed and
  PhotoCraft may read the Adobe colour data installed on his Mac at runtime (ICC profiles, Photoshop Color Settings, Camera
  Raw profiles). Never commit Adobe's proprietary files; keep a pure-Rust fallback on every Adobe/C path.
- Still binding: never crash, everything is a command, `cargo fmt` + `cargo clippy -- -D warnings` + tests on touched crates.
- Commit and push straight to `main` on `origin` (his fork). Commit only your own paths (`git commit -- <paths>`):
  parallel agents share this checkout.
- Colour vs Photoshop is measured, not guessed: `tools/ps-compare` (PSD layers rendered by PhotoCraft vs the composite
  Photoshop stored in the file, CIEDE2000).
