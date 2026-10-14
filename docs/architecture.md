# Architecture

> **Last reviewed:** 2026-10-14 · **Change:** first version (M0) · **Target:** Foundry Nuke (current release)

NukeCraft is a stack of small Rust crates. The engine knows nothing about the user interface; the
command-line tool, the MCP server and (from M1) the desktop app all drive the same `Session` through
the same command registry.

## 1. Crates and layering

Dependencies only point downward, or along the same-layer edges listed below. `cargo xtask layers`
enforces this and fails if a crate below L5 depends on egui, eframe, winit, rfd, cpal or muda.
Everything in L0–L4 builds for `wasm32-unknown-unknown` (`cargo xtask wasm`).

| Layer | Crate (`nukecraft-…`) | Responsibility |
|---|---|---|
| L0 | `time` | `FrameTime` (720 720 subframes per frame), `Fps` (rational, NTSC-aware), `FrameRange`, frame-range expressions (`1-10x2,15`), `Tick` interop with the sibling apps |
| L0 | `geom` | `V2`, `Affine`, `IRect` (half-open pixel boxes: bbox and regions, `clamp_into` for edge-repeat reads), `Format` and Nuke's named formats |
| L0 | `color` | Transfer functions (sRGB, Rec. 709, Gamma 1.8/2.2, Cineon), Rec. 709 luminance, the 29 Merge operations |
| L1 | `raster` | `Image`: any number of named channels (`rgba.red`, `depth.Z`, `layer.chan`) over a bbox, rows bottom-up, edge-repeat sampling, size and channel limits |
| L1 | `curve` | Animation curves: keys on `FrameTime`, constant / linear / horizontal / Catmull-Rom segments, constant extrapolation |
| L2 | `graph` | The script: `Root` settings, nodes (stable `NodeId`, unique name, class, knobs, inputs, DAG position), cycle-refusing connections, the `.ncs` file with a lenient, repairing loader |
| L2 | `nodes` | The node registry (`NodeSpec`: class, inputs, knobs, `info`/`request`/`engine`/`input_time`), knob validation and evaluation, resampling filters, the built-in nodes |
| L3 | `render` | Pull-based evaluation: info and region-of-interest propagation, per-render memoisation, cycle and depth limits, the render thread, display conversion |
| L3 | `io` | PNG and OpenEXR in and out (pure Rust), frame-number patterns, the Read and Write nodes, `execute_write` |
| L4 | `engine` | `Session`: script, undo/redo, journal, crash guard, the command registry, the demo script |
| L5 | `automation` | The MCP server (stdio JSON-RPC): `execute`, `list_commands`, `node_classes`, `get_script`, `render_image` |
| L5 | `fidelity` | Synthetic cases measured by NukeCraft and (through a generated Python script) by Nuke, and their comparison |
| L5 | `ui-egui` | The desktop interface (egui): menus from the command registry, shortcuts, panels, dialogs, the automation registry, and `snapshot` (headless runs with a CPU rasteriser) |
| L6 | apps `nukecraft-cli`, `nukecraft` | Command-line tool (render, exec, info, MCP); the desktop app (eframe, OpenGL) |

Allowed same-layer edges: `nodes → graph`, `io → render`.

## 2. Time

A script's time is in **frames**, as Nuke's is: keys sit on frame numbers and changing the frame
rate never moves a key. `FrameTime` counts subframes, 720 720 per frame (lcm of 1‥16), so motion-blur
and retime offsets in halves … sixteenths of a frame are exact. Files hold frame numbers (`12`,
`12.5`), which round-trip exactly. `Fps` matters only where seconds matter (media, audio); `Tick`
(254 016 000 000/s, shared with FilmCraft and EffectCraft) converts whole frames exactly at every
broadcast rate. See [plan/adr/0001-frame-time.md](../plan/adr/0001-frame-time.md).

## 3. Image model

See [plan/adr/0002-image-model.md](../plan/adr/0002-image-model.md).

- **Pixel grid:** origin at the bottom-left of the format, y up, pixel `(x, y)` covers
  `[x, x+1) × [y, y+1)` with its centre at `(x + 0.5, y + 0.5)`. Files with top-down rows (PNG, EXR)
  are flipped on the way in and out.
- **Format vs bbox:** the format is the display window (size and pixel aspect); the bbox is where a node
  has data. A Blur's bbox grows past the format unless cropped; a Transform's follows the transformed
  image; a Merge's is the union (or intersection, A, B).
- **Outside the bbox the edge pixels repeat.** A node that needs black outside its data adds a
  one-pixel black border (Crop's and Transform's "black outside"). `IRect::clamp_into` gives the part
  of a bbox that answers every read in a region under this rule, which is what the renderer fetches.
- **Values:** 32-bit float, linear light, premultiplied alpha. Read converts from the file's colour
  space (sRGB for PNG, linear for EXR, or the `colorspace` knob); Write converts back. As in Nuke, Read
  does not premultiply: unpremultiplied files go through a Premult node.
- **Channels:** any set of `layer.channel` names, `rgba` first. Nodes declare which they process
  (`channels` knobs); the rest pass through.

## 4. The script

```
Script { root: Root { format, fps, frames }, nodes }
Node   { id, name, class, knobs: { name → KnobValue }, inputs: [Option<NodeId>], x, y, disabled, label }
KnobValue = Bool | Number(Anim) | Vector([Anim]) | Text      Anim = Static(f64) | Curve
```

Only knobs that differ from their default are stored; defaults come from the class (`KnobSpec`),
except format-dependent ones (Transform `center`, Crop `box`), which are fixed when the node is made,
as in Nuke. Knob values are validated against their kind (`float`, `xy`, `color`, `box`, `choice`,
`channels`, `format`…) when set; when rendering, an invalid stored value reads as the default.

Connections use ids, so renaming never breaks a pipe; `connect` refuses cycles. The `.ncs` file is the
script as JSON headed by `savedBy` and `schema` ([script-format.md](script-format.md)). Loading
repairs instead of trusting: duplicate ids and names, dangling inputs, cycle-closing connections and
invalid knob values are fixed and reported as warnings. A newer `schema` is refused rather than
misread. Saving checks that the text reads back, then writes atomically (temporary file + rename).

## 5. Rendering

A node class is three functions, mirroring a Nuke operator (and optionally a fourth):

- `info(ctx, input infos) → ImageInfo { format, bbox, channels, frames }` (Nuke's validate);
- `request(ctx, own info, input infos, region, input) → region` — what part of an input is needed;
- `engine(ctx, own info, inputs, region) → Image` — the pixels of exactly `region`;
- `input_time(ctx, input, input frame range) → FrameTime` — for time nodes (FrameHold,
  TimeOffset): the time an input is read at; without it inputs are read at the node's own time.

`Renderer::render(node, t, region)` computes the node's info, clamps `region` into its bbox
(`clamp_into`), asks each connected input for its request, renders the inputs recursively, then runs
the engine. Infos and images are memoised per `(node, t[, region])` within one renderer, so diamonds
render once. Unconnected inputs read as black with the root format (mask inputs as "no mask");
disabled nodes pass input 0 through. Walks are cycle-checked and limited to `MAX_DEPTH` (512) nodes;
callers render on `with_render_stack` (a 64 MB thread), which also turns an escaped panic into an
error.

Not yet: tiles and threads, a cache that survives edits, GPU, proxy mode, motion blur, multiview
([gaps.md](gaps.md)).

## 6. Commands, sessions and agents

`Session` holds the script, its path, the registry (built-ins + Read/Write), the current frame,
undo/redo stacks of script snapshots and a journal. `Session::execute(id, params)`:

1. looks the command up and checks `enabled()`;
2. runs it inside `catch_unwind`;
3. on success, if the command is undoable and changed the script, pushes one undo step named after it;
4. on failure or panic, restores the script as it was.

Commands are `area.verb` ids (`node.create`, `node.setKnob`, `render.execute`, `query.pixel`…), each
with a label, menu path, shortcut and a parameter description; `query.commands` lists them and the
MCP server publishes them. The CLI's subcommands and the MCP `execute` tool are thin layers over
`execute`. See [agents.md](agents.md).

## 7. The desktop app

`nukecraft-ui-egui` draws the whole window from `NukecraftApp::ui`: menus built from the command
registry (each command's `menu` path) and the node registry (one Nodes submenu per category), the
Viewer (centre), the Node Graph (bottom) and the Properties panel (right). The UI never edits the
script itself: every change is `Session::execute` with a command id, so it is validated, atomic and
undoable, and an agent could have done the same. Gestures that pass through many values (dragging a
number, moving nodes, inserting a node and rewiring) are one undo step through undo groups
(`Session::begin_group` / `end_group`).

The Viewer renders on its own thread with a 64 MB stack and its own node registry. Requests carry a
copy of the script; only the newest waiting request is rendered. Finished frames are cached by
(script revision, node, time), so any edit invalidates them and nothing stale is shown.

The bottom pane holds the Node Graph and the Curve Editor as tabs. The Curve Editor shows the
animated knob components of the nodes whose properties are open and edits them through
`node.editKeys` (one component's curve: remove and set keys in one atomic command) and
`node.setInterpolation`; a key drag is one undo group. Bare-key shortcuts go to the panel under the
pointer (Delete over the Curve Editor deletes keys, not nodes).

Copy, cut and paste go through `edit.copy`, `edit.cut` and `edit.paste`: a fragment is JSON with
inputs by name, so pasting keeps the pipes inside it and reconnects to outside nodes of the same
name; the app puts it on the system clipboard (which arrives as Copy/Cut/Paste events, not keys)
and keeps the last copy for Edit ▸ Paste.

Render… (and a Write node's Render button) runs the same plan as the `render.execute` command
(`nukecraft_engine::render_plan`) on a background thread from a copy of the script, frame by frame,
with a progress window and Cancel; the window stays responsive and edits made meanwhile don't
affect the render. The command itself stays synchronous for the CLI, MCP and the control channel.

Every interactive element registers an automation id and rect each frame. The control channel
(`--control PORT`) and the headless harness (`snapshot::Headless`) click, drag and type by id; the
harness also rasterises egui's triangles on the CPU, which lets agents and tests look at the UI
without a display or GPU (it matches the real window, checked under Xvfb).
