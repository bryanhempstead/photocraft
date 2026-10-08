# Driving PhotoCraft from devices and scripts

PhotoCraft is driven by **keystrokes** (device profiles), by **commands** over the control channel
(scripts), or both. Every key below can be changed in Edit › Keyboard Shortcuts (Preferences ›
`shrt.`), and the user keymap lives in `<settings>/keymap.json`
(`~/Library/Application Support/Photocraft/keymap.json` on a Mac).

The macOS app bundle id is **`ai.storyteller.photocraft`** (`packaging/macos/Info.plist.in`), so
Logi Options+ / Loupedeck and other per-app device software can target PhotoCraft on its own.

## Logi MX Creative Console (Loupedeck70 keypad, 71 dialpad, 72 actions ring)

Bryan's Photoshop profiles use the Photoshop plug-in actions on the left. A PhotoCraft profile sends
the key in the last column (Logi Options+: action "Keyboard shortcut"; dials: one key per direction).
`F13`–`F17` and `⌃⌥⇧`+letter are device-only keys nothing else binds.

| Photoshop action (Logi) | PhotoCraft command | Key |
|---|---|---|
| FitOnScreen | `view.fitOnScreen` | ⌘0 |
| Save | `file.save` | ⌘S |
| ActivateMoveTool | `tools.select.move` | ⌃⌥⇧V (or V) |
| BrushActionsDynamicFolder | (Logi folder of brush actions) | use the rows below |
| ActivateSpotHealingBrushTool | `tools.select.spotHealing` | ⌃⌥⇧J |
| FreeTransform | `edit.freeTransform` | ⌘T |
| ActivateQuickSelectTool | `tools.select.quickSelection` | ⌃⌥⇧Q |
| BrushSize (dial) | `tools.decreaseBrushSize` / `tools.increaseBrushSize` | [ / ] |
| ActivateLassoTool | `tools.select.lasso` | ⌃⌥⇧L |
| BrushOpacity (dial) | `tools.opacityDown` / `tools.opacityUp` | ⇧F15 / ⇧F16 |
| RoundBrushHardness (dial) | `tools.decreaseBrushHardness` / `tools.increaseBrushHardness` | ⇧[ / ⇧] |
| AdjustmentLayerLevelsCreate | `layer.newAdjustmentLayer.levels` | F13 |
| LayerOpacity (dial) | `tools.layerOpacityDown` / `tools.layerOpacityUp` | ⇧F13 / ⇧F14 |
| ChangeBlendMode (dial) | `tools.blendModePrevious` / `tools.blendModeNext` | ⇧- / ⇧= |
| LayerCreate | `layer.new.layer` | ⌘⇧N |
| LayerDuplicate | `layer.new.layerViaCopy` | ⌘J |
| createLayerMaskFromSelection | `layer.layerMask.revealSelection` | F14 |
| ActivateCropTool | `tools.select.crop` | ⌃⌥⇧C |
| Liquify | `filter.liquify` | ⌘⇧X |
| ExportAs | `file.export.exportAs` | ⌘⌥⇧W |
| Undo / Redo | `edit.undo` / `edit.redo` | ⌘Z / ⌘⇧Z |
| Space (generic action) | Hand tool (hold) | Space |
| ZoomInOut (dial) | `view.zoomOut` / `view.zoomIn` | ⌘- / ⌘= |
| Navigate Layer List (dial) | `layer.selectBelow` / `layer.selectAbove` | ⌥[ / ⌥] |
| ActivatePatchSelection | `tools.select.patch` | ⌃⌥⇧P |
| openHueSaturationDialog | `image.adjustments.hueSaturation` | ⌘U |
| AdjustImageExposureValue | `image.adjustments.exposure` (opens the dialog; no dial step yet) | F15 |
| ContentAwareFill | `edit.contentAwareFill` | F16 |
| GenerativeFill | — (no generative fill in PhotoCraft) | — |
| ActivateMagicWandTool | `tools.select.magicWand` | ⌃⌥⇧W |
| ActivateMagicLassoTool | `tools.select.magneticLasso` | ⌃⌥⇧G |
| (extra) New Hue/Saturation layer | `layer.newAdjustmentLayer.hueSaturation` | F17 |

Every toolbar tool also has `tools.select.<tool>` (no default key; bind one in `shrt.`).
Mouse side buttons (MX Master back / forward) bind to any command in `shrt.` › `mouse.`
(`mouseButtons` in keymap.json: `{"MouseBack": "edit.undo"}`).

## Script bridge (control channel)

Start PhotoCraft with `--control <port> --control-token-file <file>` plus
`--automation-read-root <dir>` (and `--automation-write-root <dir>` for saves). Requests are JSON lines
(`docs/control-protocol.md`); the first one authenticates with the token.

| Script use | Method | Notes |
|---|---|---|
| shotboard `psin`: place into the open document as a Smart Object | `app.place {"path": "<relative to read root>", "scale"?, "fit"?, "center"?}` | `file.placeEmbedded {path}` is refused over the control channel (ambient paths); `app.place` reads through the read root. Needs an open document. |
| shotboard `psnew`: open as a new document | `app.open {"path": "<relative to read root>"}` | |
| hlpr: current document path | `ui.inspect {}` → `result.session.documents[result.session.active].path` | `null` for an unsaved document |
| run any menu command | `ui.menu.invoke {"id": …}` / `engine.execute {"command": …, "params": …}` | |

The read root must contain the files (e.g. launch with `--automation-read-root ~/` or the shotboard
folder); paths are relative to it and `..` is refused.
