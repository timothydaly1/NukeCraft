# Where NukeCraft falls short

> **Last reviewed:** 2026-10-14 · **Change:** M2.2 (channel nodes) · **Target:** Foundry Nuke (current release)

An honest list of how far NukeCraft is from being a replacement for Nuke, ranked by what blocks real
work first. Read it before choosing work. Numbers marked "≈" are judgements, not measurements; how
they were reached is in [target-app-parity.md](target-app-parity.md).

## The short version

NukeCraft is **pre-alpha**. The engine core is solid for what it covers — a node graph, 26 node
classes, PNG/EXR in and out, exact time, animation, undo, a CLI and an MCP server — and there is a
first desktop app (Node Graph, Viewer, Properties). But the node library is ≈ 10% of Nuke's everyday
set, nothing has been compared with Nuke, and Nuke scripts (`.nk`) can't be opened. Nobody can do
production work in it yet.

## Ranked gaps

| # | Gap | Why it matters | Milestone |
|---|---|---|---|
| G1 | **Desktop app is a first version**: no Curve Editor or Dope Sheet, no on-viewer handles (Transform, Ramp, Crop), no colour picker, no copy/paste of nodes, Groups or Backdrops, no Dot insertion on a pipe, fixed panel layout (no docking), no recent files or preferences; Render… blocks the window while it writes; the Viewer shows only the format (not the bbox outside it), no wipe/compare or ROI | Artists feel these within minutes | M1 follow-ups, M2 |
| G2 | **Node library** (≈ 26 of the ≈ 250 nodes in Nuke's toolbar; no Reformat, Roto, Keymix, Tracker, Keyer, Shuffle2's mapping UI…) | Real scripts use 30–60 distinct classes | M2, M5–M8 |
| G3 | **No fidelity measurements yet**: the harness is ready (161 cases, [fidelity/README.md](fidelity/README.md)) but Nuke's numbers haven't been captured | "Looks right" isn't "matches Nuke"; artists notice filter and blur differences | needs a Nuke session (Non-commercial is enough) |
| G4 | **No `.nk` import or export** | Existing work and studio pipelines live in `.nk` | M3 |
| G5 | **No expressions or scripting** (TCL expressions, linked knobs, Python) | Rigs, gizmos and pipeline tools depend on them | M3 |
| G6 | **Performance**: single-threaded, whole-region renders, no persistent cache, no GPU, no proxy mode | 2K/4K plates with deep graphs need tiles, threads and caching to be interactive | M4 |
| G7 | **Colour management**: six built-in transfer functions; no OCIO configs, LUT files, viewer process | Every studio pipeline is OCIO-based | M5 |
| G8 | **Media**: PNG and EXR only; no DPX, TIFF, Cineon, JPEG, video | Plates arrive as DPX/EXR sequences and ProRes/H.264 | M9 (video via FilmCraft crates) |
| G9 | **Motion blur, multiview, metadata, deep, 3D, particles** | Needed for feature and episodic work | M6–M8 |
| G10 | **Plug-ins**: no OFX host, no gizmos/groups, no BlinkScript equivalent | Studios depend on third-party tools | M10 |

## What is already better than "early"

- **Never-crash discipline from day one:** every crate denies panicking shortcuts, inputs are capped,
  graph walks are bounded, commands are atomic and guarded.
- **Agent control:** every operation is a command reachable over MCP and the CLI, with a PNG view of
  any node for agents to look at.
- **Readable scripts:** `.ncs` is diffable JSON with a repairing loader.
