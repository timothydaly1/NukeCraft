# Status

> **Updated:** 2026-10-14

- **Milestone:** M2 — everyday nodes — in progress. Work is on branch `m2-everyday-nodes` (stacked on
  `m1-desktop-app`; neither is merged to `main` yet).
- **Waiting on the maintainer:** a Nuke session (Non-commercial is enough) to capture the oracle:
  `docs/fidelity/README.md` → "Measuring with Nuke". Until then nothing is checked against Nuke.
- **Next task:** the rest of M2: CheckerBoard's colours and lines and ColorBars (M2.11), Group with
  Input and Output (M2.12), then Roto (M2.13); the other G1 follow-ups in
  `docs/gaps.md`. When the oracle arrives, fidelity fixes come first.
- **Gate:** `cargo xtask ci` green (162 tests, including headless UI and fidelity tests).
- **Looking at the UI:** `cargo run -p nukecraft-ui-egui --example snapshot -- --out ui.png --step …`
  (headless, no GPU). The real window runs under Xvfb with Mesa's software GL; drive it with
  `--control PORT` (docs/control-protocol.md).
- **Blockers:** none. Open decision for the maintainer: the project name (see README's trademark note).
- **Running agents:** none.

## Recently done

- M2.10: Backdrop (BackdropNode) and StickyNote in the Node Graph (a backdrop carries its nodes,
  resizes, wraps the selection); copy, cut and paste (`edit.copy`/`cut`/`paste`, the system
  clipboard); a label field on every node; synthetic Shift/Cmd clicks hold their modifiers.
- M2.9: the Curve Editor (a tab beside the Node Graph): curves of open nodes' animated knob
  components, select/box-select/move keys (whole frames), interpolation, delete, fit; engine
  commands `node.editKeys` and `node.setInterpolation`.
- M2.8: on-viewer handles for the nodes whose properties are open: Transform (translate, rotation
  arm), CornerPin2D (to1..4), Rectangle, Radial, Crop (box corners and centre), Ramp (p0, p1); one
  undo step per drag, animated knobs keyed at the current frame; `ui.drag` on the control channel.
- M2.7: Render… and a Write's Render button run in the background (progress window, Cancel; the
  same plan as `render.execute`); `app.state` reports the progress.
- M2.6: Sharpen, Erode (fast) and (filter), Glow, Median, NoOp; Blur gains a mask input; blurs and
  erodes read past the image's edge in constant time; 14 more fidelity cases (F20).
- M2.5: time nodes FrameHold, TimeOffset, FrameRange; the renderer reads inputs at the times a node
  asks for (`NodeSpec::input_time`); a FrameHold made at frame f holds frame f; 6 more fidelity
  cases (F19).
- M2.4: Reformat, Mirror (Mirror2), Position, CornerPin2D; Transform's resampler shared by every
  warp; a homography type in `nukecraft-geom`; 23 more fidelity cases (F18).
- M2.3: Rectangle and Radial (Draw menu, with Ramp), Keymix, Dissolve; a format-relative knob default
  (`area`); 18 more fidelity cases (F17).
- M2.2: channel nodes: classic Shuffle, Copy (K), ChannelMerge, Remove; channels inside any layer
  order red, green, blue, alpha first; Nuke's built-in layers (`depth`, `motion`, `forward`,
  `backward`, `mask`) parse before an image has them; over the Node Graph, J/K/L go to the graph
  (K is Copy), not to playback; 22 more fidelity cases (F16).
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
