# Execution plan

Milestones in order. Each task has an id used in its commit message (`M1.2: node graph panel`).
A task is done when its tests pass, `cargo xtask ci` is green, and the docs it touches
(`docs/gaps.md`, `docs/fidelity/README.md`, `plan/nuke/node-catalog.md`, `plan/STATUS.md`) are updated.

## M0 — Foundation ✅

| Id | Task | State |
|---|---|---|
| M0.1 | Workspace, licences, rules (AGENTS.md, CLAUDE.md), xtask (layers, assets, wasm, ci), CI workflow | done |
| M0.2 | L0: frame time, geometry and formats, colour transfer functions and Merge operations | done |
| M0.3 | L1: multichannel images with bbox and edge repeat; animation curves | done |
| M0.4 | L2: script model and `.ncs` with a repairing loader; node registry with Constant, CheckerBoard, Ramp, Grade, Saturation, Premult, Unpremult, Merge, Transform, Blur, Crop, Dot, Switch | done |
| M0.5 | L3: pull renderer (info, request, engine; memo; depth and cycle limits); PNG/EXR; Read/Write | done |
| M0.6 | L4–L6: Session with undo, journal, crash guard and command registry; demo; MCP server; CLI | done |

## M1 — The desktop app

| Id | Task |
|---|---|
| M1.1 | `crates/ui-egui` and `apps/nukecraft`: window, panels, menus built from the command registry, shortcuts, file dialogs, autosave, Project Settings, undo groups, the automation registry and the headless snapshot tool (moved here from M1.5: every later task needs it to look at the UI) — done |
| M1.2 | Node Graph panel: nodes and pipes, create (Tab search, toolbar menus, shortcuts B/G/M/T/./R/W), connect by dragging, select/move/delete/disable, zoom/pan, labels — done (Dot insertion on a pipe: later) |
| M1.3 | Viewer: render the viewed node on a background thread, format and bbox overlays, channel and alpha display, exposure/gamma, zoom/pan, pixel readout, frame slider and playback with a RAM cache |
| M1.4 | Properties panel: a widget per knob kind (sliders, colour, XY with on-viewer handles, choices, channels, file browser), key/animated indicators, set-key and no-animation actions |
| M1.5 | Control channel (JSON lines over TCP: execute, widget automation ids) and the MCP `--bridge` |
| M1.6 | Fidelity harness: synthetic case generator, oracle number files, comparison tests; first cases F1–F3 |

## M2 — Everyday compositing nodes

Reformat, Shuffle (Shuffle2), Copy, ChannelMerge, Remove, ColorCorrect, Clamp, Invert, Multiply/Add/Gamma,
Exposure, HueCorrect, Keymix, Dissolve, FrameHold, TimeOffset, FrameRange, Rectangle, Radial, Roto (shapes,
feather, animation), CornerPin2D, Mirror, Position, Sharpen, Erode/Dilate (FilterErode), Defocus, Glow,
EdgeDetect, NoOp, Backdrop, StickyNote, Group (with Input/Output), the remaining CheckerBoard colours and lines.

## M3 — Scripts and expressions

`.nk` import (tokenizer for the TCL-flavoured syntax, node stack semantics, knob value forms, user knobs),
mapping of known classes and knobs with a report of the rest; `.nk` export; TCL expression subset
(`frame`, arithmetic, `curve`, knob references), linked knobs; a scripting language decision (ADR).

## M4 — Performance

Tile-based multi-threaded rendering (rayon on native), a cache that survives edits (keyed by a hash of
each node's upstream inputs), viewer playback cache, proxy mode, a wgpu path for the heavy filters
(checked against the CPU path), a benchmark suite (`cargo xtask bench`).

## M5 — Colour

OCIO config reading (built-in transforms, file transforms: .cube, .spi1d/.spi3d, CDL), OCIOColorSpace,
OCIODisplay, Viewer process, Log2Lin, ColorLookup, Colorspace node.

## M6 — Tracking and warping

Tracker (2D point tracking, stabilise/match-move outputs), CornerPin from tracks, STMap, IDistort,
SplineWarp, GridWarp, LensDistortion, motion blur on Transform and CornerPin.

## M7 — Keying

Keyer, HueKeyer, ChromaKeyer, IBKColour/IBKGizmo-style keying, Difference, despill, Cryptomatte read.

## M8 — Deep, 3D, multiview

Deep image model, DeepRead/DeepMerge/DeepToImage/DeepHoldout; Card/Camera/Axis/Scene/ScanlineRender,
ReadGeo (OBJ, glTF); multiview (stereo) and metadata.

## M9 — Media

DPX, Cineon, TIFF, JPEG; video Read/Write through FilmCraft's pure-Rust codecs pinned as git
dependencies (H.264, ProRes, …), as EffectCraft does.

## M10 — Ecosystem

Gizmos (Groups saved as files), WebAssembly plug-ins (sandboxed, like EffectCraft's), an OFX host
decision (OFX plug-ins are native code: an out-of-process host keeps `unsafe` out of the workspace; ADR).
