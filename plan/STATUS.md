# Status

> **Updated:** 2026-10-14

- **Milestone:** M2 — everyday nodes — in progress. Work is on branch `m2-everyday-nodes` (stacked on
  `m1-desktop-app`; neither is merged to `main` yet).
- **Waiting on the maintainer:** a Nuke session (Non-commercial is enough) to capture the oracle:
  `docs/fidelity/README.md` → "Measuring with Nuke". Until then nothing is checked against Nuke.
- **Next task:** M2.2 channel nodes (Shuffle, Copy, ChannelMerge, Remove), then M2.3 Keymix,
  Dissolve, Rectangle, Radial; M2.4 Reformat, Mirror2, Position, CornerPin2D; M2.5 time nodes;
  M2.6 filters. The G1 follow-ups in `docs/gaps.md` (Curve Editor, on-viewer handles, render
  progress) come after. When the oracle arrives, fidelity fixes come first.
- **Gate:** `cargo xtask ci` green (123 tests, including headless UI and fidelity tests).
- **Looking at the UI:** `cargo run -p nukecraft-ui-egui --example snapshot -- --out ui.png --step …`
  (headless, no GPU). The real window runs under Xvfb with Mesa's software GL; drive it with
  `--control PORT` (docs/control-protocol.md).
- **Blockers:** none. Open decision for the maintainer: the project name (see README's trademark note).
- **Running agents:** none.

## Recently done

- M2.1: colour nodes. Shared colour-node machinery (mask input 1 with `maskChannelInput` and
  `invert_mask`, `unpremult`, `mix`) on Grade and Saturation (plus its four luminance formulas);
  Clamp, Invert, Multiply, Add, Gamma, Exposure (`EXPTool`, stops), ColorCorrect (master and
  shadows/midtones/highlights); Merge's `invert_mask`; mask pipes enter nodes from the right in the
  Node Graph; 31 more fidelity cases (F13–F15).

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
