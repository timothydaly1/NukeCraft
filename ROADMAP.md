# NukeCraft Roadmap

**Stage: pre-alpha** · foundation (M0) and first desktop app (M1.1–M1.5) done · next: fidelity harness (M1.6), everyday nodes (M2)

> **Last reviewed:** 2026-10-14 · **Target:** Foundry Nuke (current release)

NukeCraft aims to do what Nuke does — node-based compositing with the same nodes, knobs and
behaviour — written from scratch in pure Rust, built the way the ArtCraft Crafting Apps are built.
This page is the one-page summary; the evidence is in [docs/target-app-parity.md](docs/target-app-parity.md),
the work list in [docs/gaps.md](docs/gaps.md), and the plan in [plan/execution-plan.md](plan/execution-plan.md).

## Headline numbers

| | Value | Kind |
|---|---|---|
| Node library breadth | ≈ 17% (43 classes) | Counted against [the node catalog](plan/nuke/node-catalog.md) |
| Ready for real work | ≈ 8% | First desktop app; 43 nodes; no `.nk` |
| Behaviours checked against Nuke | 0 of 20 listed | [docs/fidelity/README.md](docs/fidelity/README.md) |
| Tests | 152 | `cargo test --workspace` (including headless UI tests) |
| Agent control | every command | MCP and CLI |

## Milestones

| Milestone | What it brings | State |
|---|---|---|
| M0 Foundation | Layered crates, image model, script format, 15 nodes, PNG/EXR, undo, CLI, MCP | done |
| M1 Desktop app | Node Graph, Viewer, Properties, timeline, control channel, fidelity harness | done |
| M2 Everyday nodes | Reformat, Shuffle, ColorCorrect, Roto, Keymix, CornerPin, Erode, Defocus, Group… | in progress: M2.1 colour, M2.2 channel, M2.3 draw and mix, M2.4 transform, M2.5 time, M2.6 filter nodes done |
| M3 Scripts | `.nk` import/export, expressions, linked knobs | |
| M4 Performance | Tiles, threads, persistent cache, GPU, proxy | |
| M5 Colour | OCIO, LUTs, viewer process | |
| M6 Tracking and warping | Tracker, STMap, SplineWarp, motion blur | |
| M7 Keying | Keyer, HueKeyer, IBK-style, Cryptomatte | |
| M8 Deep, 3D, views | Deep compositing, 3D scene and ScanlineRender, stereo | |
| M9 Media | DPX, TIFF, video via FilmCraft's codecs | |
| M10 Ecosystem | Gizmos, WebAssembly plug-ins, OFX host decision | |
