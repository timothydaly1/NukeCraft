# Where NukeCraft falls short

> **Last reviewed:** 2026-10-14 · **Change:** first version (M0) · **Target:** Foundry Nuke (current release)

An honest list of how far NukeCraft is from being a replacement for Nuke, ranked by what blocks real
work first. Read it before choosing work. Numbers marked "≈" are judgements, not measurements; how
they were reached is in [target-app-parity.md](target-app-parity.md).

## The short version

NukeCraft is **pre-alpha**. The engine core exists and is solid for what it covers — a node graph,
13 node classes, PNG/EXR in and out, exact time, animation, undo, a CLI and an MCP server — but there
is **no desktop app**, the node library is ≈ 5% of Nuke's everyday set, nothing has been compared
with Nuke, and Nuke scripts (`.nk`) can't be opened. Nobody can do production work in it yet.

## Ranked gaps

| # | Gap | Why it matters | Milestone |
|---|---|---|---|
| G1 | **No desktop app** (Node Graph, Viewer, Properties, Curve Editor, timeline) | A compositor is used visually; agents and the CLI alone don't make it usable | M1 |
| G2 | **Node library** (≈ 13 of the ≈ 250 nodes in Nuke's toolbar; no Reformat, Shuffle, ColorCorrect, Roto, Keymix, Tracker, Keyer…) | Real scripts use 30–60 distinct classes | M2, M5–M8 |
| G3 | **No fidelity measurements** ([fidelity/README.md](fidelity/README.md) lists 12 unchecked behaviours) | "Looks right" isn't "matches Nuke"; artists notice filter and blur differences | M1.6 onwards |
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
