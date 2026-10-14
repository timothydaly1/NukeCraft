# Status

> **Updated:** 2026-10-14

- **Milestone:** M1 — the desktop app — done, including M1.6 (the fidelity harness). Work is on
  branch `m1-desktop-app`, not yet merged to `main`.
- **Waiting on the maintainer:** a Nuke session (Non-commercial is enough) to capture the oracle:
  `docs/fidelity/README.md` → "Measuring with Nuke". Until then nothing is checked against Nuke.
- **Next task:** M2 (everyday nodes), with the G1 follow-ups in `docs/gaps.md` (Curve Editor,
  on-viewer handles, render progress). When the oracle arrives, fidelity fixes come first.
- **Gate:** `cargo xtask ci` green (115 tests, including headless UI and fidelity tests).
- **Looking at the UI:** `cargo run -p nukecraft-ui-egui --example snapshot -- --out ui.png --step …`
  (headless, no GPU). The real window runs under Xvfb with Mesa's software GL; drive it with
  `--control PORT` (docs/control-protocol.md).
- **Blockers:** none. Open decision for the maintainer: the project name (see README's trademark note).
- **Running agents:** none.

## Recently done

- M1.6: fidelity harness (`crates/fidelity`): 108 synthetic cases (F1–F3, F6–F10, F12), a generated
  Nuke Python script written for Non-commercial's limits (tested against a mock `nuke` module),
  `nukecraft-cli fidelity export | run | compare`, a Markdown report including knob-default
  differences, and a regression test over `docs/fidelity/matching.txt` once the oracle exists.

- M1.5: control channel (`nukecraft --control PORT`: engine.execute, app.state, app.view,
  ui.inspect, ui.click, ui.key, ui.text, ui.screenshot) and `nukecraft-cli mcp --bridge PORT`;
  verified end to end against the real window under Xvfb.
- M1.4: Properties panel: a widget per knob kind, drags as one undo step, key buttons, rename,
  disable, file browsing, Render on Write.
- M1.3: Viewer: render thread, revision-keyed cache, channels/exposure/gamma, overlays, pixel
  readout, timeline and playback.
- M1.2: Node Graph: draw, select, move, connect, Tab search, node keys that insert/branch.
- M1.1: app shell: menus from the command registry, shortcuts, dialogs, autosave, undo groups,
  automation registry, headless snapshots.
- M0.1–M0.6: workspace and rules; L0–L6 crates; 15 node classes; PNG/EXR; CLI; MCP server; demo.
