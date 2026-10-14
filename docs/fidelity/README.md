# Fidelity: checking NukeCraft against Nuke

> **Last updated:** 2026-10-14 · **Harness:** ready (225 cases) · **Oracle numbers:** none yet

NukeCraft's tests pin its own behaviour; they can't show it matches Nuke. The fidelity harness
(`crates/fidelity`) can: 225 synthetic cases (tiny graphs with Nuke's class and knob names on a
64×64 format) are measured by NukeCraft and by Nuke, and compared value by value. Nuke's side is a
generated Python script that writes one JSON file, the **oracle**; nothing else from Nuke is used
or kept.

## Measuring with Nuke (Non-commercial is enough)

The script is written for Nuke Non-commercial's limits: each case has at most a handful of nodes
(its Python API retrieves only 10 nodes per script), nodes are kept by reference and never looked
up, nothing is rendered to disk, and it runs in the GUI's Script Editor (command-line rendering is
restricted to encrypted scripts). Non-commercial's licence is for non-commercial use: check that
measuring behaviour for this open-source project fits its terms before you start.

1. Write the script: `cargo run -p nukecraft-cli -- fidelity export plan/nuke/ref/`
   (`plan/nuke/ref/` is gitignored).
2. Start Nuke (Non-commercial: `Nuke… --nc`), open a new empty script, open **Script Editor** and run
   `exec(open('/full/path/to/plan/nuke/ref/nukecraft_fidelity.py').read())`. It takes seconds and
   prints where it wrote `nukecraft_oracle.json` (your home folder when run this way; set
   `NUKECRAFT_ORACLE_OUT` to choose).
3. Copy that file to `docs/fidelity/oracle.json` and compare:
   `cargo run -p nukecraft-cli -- fidelity compare docs/fidelity/oracle.json --report docs/fidelity/report.md --matching docs/fidelity/matching.txt`
4. Commit `oracle.json`, `report.md` and `matching.txt`. From then on `cargo test` fails if a case that
   matched stops matching. Cases that differ are the fidelity work list; fix one, re-run step 3.

The oracle holds only numbers measured on these synthetic graphs, plus the default values of the
knobs NukeCraft implements (Nuke's own defaults, which the report compares with NukeCraft's). Cases
for features NukeCraft doesn't have yet (Notch/Lanczos/Sinc filters, soft-light, Exposure in
densities and Cineon units) are measured too, so the numbers are waiting when the feature is built.

`nukecraft-cli fidelity run` prints NukeCraft's own numbers in the same format, for inspection.

## The cases

| Area | Cases | What they pin down |
|---|---|---|
| F1 | `transform.filter.*` (11), `transform.minify.*` (3) | Each filter's kernel (an impulse moved by a fraction of a pixel), and how kernels widen when scaling down |
| F2 | `transform.matrix.*` (4) | The order of translate/rotate/scale/skew about the centre (a coordinate image through the Impulse filter) |
| F3 | `blur.*` (7) | Size → kernel for box/triangle/quadratic/gaussian; edges |
| F6 | `animation.default` | The default interpolation of new keys, read every half frame |
| F7 | `grade.*` (7), `saturation.*` (6), `premult`, `switch.which` | Grade's formula, gamma on negatives, reverse, clamps, mix; the four luminance formulas |
| F8 | `merge.*` (62) | Every operation, in range and out of range (`.hdr`); mask and mix; A's edges outside its bbox |
| F9, F10 | `checkerboard.*` (4), `colorbars.*` (2), `ramp.*` | Box placement, the four colours, lines and centre lines; the colour bars' layout and values; the four Ramp profiles |
| F12 | `crop.*` | Fractional crop boxes, the black border, reformat |
| F13 | `colorcorrect.*` (8) | ColorCorrect over a colour ramp: each control, each range, master and range together |
| F14 | `mask.*` (3), `unpremult.*` (4) | Mask input, `invert_mask` and mix on Grade and Merge; `unpremult` including a zero alpha |
| F15 | `clamp.*` (4), `invert.*` (2), `multiply`, `add`, `gamma` (2), `exposure.*` (3) | The math nodes; Clamp's clamp-to values; Gamma 0; Exposure units |
| F16 | `shuffle.*` (3), `copy.*` (5), `channelmerge.*` (12), `remove.*` (2) | Channel routing, constants, the built-in depth layer; Copy's bbox and A's edges outside its bbox; every ChannelMerge operation |
| F20 | `sharpen.*` (2), `erode_fast.*` (3), `erode_filter.*` (3), `glow.*` (3), `median.*` (2), `blur.mask` | Each filter across the edges of a square; Blur through a mask |
| F19 | `framehold.*` (2), `timeoffset.*` (4) | Held frames, increments (also before the first frame), offsets both ways, reversing with and without an offset |
| F18 | `reformat.*` (14), `mirror.*` (3), `position.fractional`, `cornerpin.*` (4) | Reformat's resize types, centring, box shape, scale, flip/flop/turn and filtering; Mirror2; Position's rounding; CornerPin2D's perspective mapping, filtering, disabled corners and invert (all on the coordinate image) |
| F17 | `rectangle.*` (6), `radial.*` (5), `keymix.*` (3), `dissolve.*` (4) | Edge antialiasing and softness; compositing over a background, replace, invert, opacity; Radial's falloff; Keymix through a mask and without one; Dissolve between three inputs |

F4 (what the Viewer and Write show outside the bbox), F5 (Edit ▸ Delete) and F11 (Read's colour
defaults per file type) need the GUI or files on disk and stay manual checks.

## Open questions

Every entry below is an implementation choice made from public documentation and general
knowledge, not yet compared with Nuke itself.

| # | Area | NukeCraft does | To check |
|---|---|---|---|
| F1 | Transform filters | Cubic family as Mitchell–Netravali (B, C): Cubic (0, 0), Keys (0, ½), Simon (0, ¾), Rifman (0, 1), Mitchell (⅓, ⅓), Parzen (1, 0); kernel widened by the local minification | The (B, C) of each name; how Nuke widens the kernel when scaling down; Notch, Lanczos4/6, Sinc4 missing |
| F2 | Transform matrix | `T(translate)·T(center)·R(rotate)·S(scale)·K(skewX, skewY)·T(−center)` | Order of skew vs scale vs rotate; skew units; `skew_order` |
| F3 | Blur | `size` = full kernel width; gaussian σ = size / 6; box, triangle, quadratic as described in `crates/nodes/src/blur.rs` | Size → kernel mapping for each filter; `crop` behaviour at the format edge |
| F4 | Outside the bbox in the Viewer and in Write | Shown and written as black | Whether Nuke's Viewer/Write show black or repeated edges there |
| F5 | Delete | Reconnects input 0's node to what the deleted node fed | Nuke's default when deleting a node in a chain |
| F6 | Animation | New keys default to Catmull-Rom; editing an animated knob keys the current frame | Nuke's default interpolation ("smooth") and its tangents |
| F7 | Grade | Gamma applied to positive values only; reverse inverts the whole formula | Gamma on negatives; clamp order with `reverse` |
| F8 | Merge | Same formula on alpha as on colour; `screen` falls back to max outside 0‥1; `divide` gives 0 on division by zero | Alpha handling per operation; `alpha masking`; `also merge` |
| F9 | CheckerBoard, ColorBars | Boxes measured from the format centre; rows alternate colours 0/1 and 2/3 (shifted by a box); lines `linewidth` wide centred on box edges, centre lines through the format centre, both antialiased; ColorBars: SMPTE-style bars at `barintensity` with a reversed strip and a -I/white/+Q/pluge strip, values written as they are | Which box gets which colour; line placement and widths; the default line widths; ColorBars' layout, values and colour space |
| F10 | Ramp | `smooth0`/`smooth1`/`smooth` as cubic ease curves; alpha ramps with the colour | The exact curves |
| F11 | Read colour | PNG defaults to sRGB, EXR to linear; `premultiplied` divides by alpha around the conversion | Default colour spaces per file type in the current Nuke colour management |
| F12 | Blur `crop`, Crop `black outside` | One-pixel black border added outside the box | Border handling with fractional boxes and `softness` |
| F13 | ColorCorrect | Per range: saturation (Rec. 709), contrast about 0.18, gamma, gain, offset; master and range controls combine (factors multiply, offsets add); range weights from the input's luminance: shadows fade out by 0.09, highlights fade in from 0.5 to 1 (smoothstep), midtones the rest | The contrast pivot; the default `lookup` curves; how master and range controls combine; which luminance |
| F14 | Masks and unpremult | `maskChannelInput` defaults to `rgba.alpha` (applies once the mask input is connected); the mask is clamped to 0..1 and multiplies `mix`; a channel the mask lacks is black; `unpremult` skips a zero alpha and leaves the alpha channel itself undivided | The default mask channel when a mask is connected from Python; mask values outside 0..1 |
| F20 | Filters | Sharpen: `v + amount·(v - gaussian(size))`, bbox grown by the blur; Erode (fast): box max (positive size) or min, whole pixels; Erode (filter): box min (positive size) or max, fractional sizes blend the neighbouring whole sizes; Glow: per channel `max(v - tolerance, 0)`, saturation about its luminance, gaussian `size`, times brightness, added; Median: a `size`-wide square window | Every formula: Sharpen's blur and clamps, the erode signs and FilterErode's filtering, Glow's tolerance (per channel or luminance) and default size, Median's size |
| F19 | Time nodes | FrameHold with `increment` holds `first_frame + n·increment`, also before `first_frame`; TimeOffset reads `t - time_offset`, then reverses over the input's range (`first + last - t`) | Reverse versus offset order; FrameHold's default `first_frame` |
| F18 | Transform family | Reformat keeps the picture's shape by matching the output's width (or height, or whichever fits/fills), centred; turn is 90 degrees counter-clockwise before scaling; flip/flop after; the bbox is clipped to the new format unless `pbb`; Position rounds to whole pixels; CornerPin2D maps from-corners to to-corners by a homography, a disabled corner stays at its from position | Reformat's `turn` direction and the order of turn/flip/flop; whether Position filters fractional moves; CornerPin2D's filtering under strong perspective |
| F17 | Draw and mix nodes | Rectangle: exact pixel coverage of a hard edge, a linear soft edge `softness` pixels wide centred on the edge; Radial: an ellipse in `area` fading from radius `1 - softness` to 1, smoothstep (linear with `plinear`); both blend `color` over the background by coverage times opacity (`replace` onto black); `area` defaults to the middle half of the format; Keymix without a mask replaces B with A | Every one of these: the soft-edge profiles, the default `area`, compositing (over or mix), Keymix's `channels` default and its behaviour without a mask |
| F16 | Channel nodes | Shuffle's sources pick the in layer's first..fourth channel; a new out layer gets red, green, blue, alpha (`depth` is `depth.Z`); Copy's `bbox` defaults to union and outside A's bbox A's edges repeat; ChannelMerge: union `a+b-ab`, intersect/multiply/in `ab`, stencil `b(1-a)`, out `a(1-b)`, absminus, plus, minus `a-b`, from `b-a`, max, min | ChannelMerge's operation list and `in`/`intersect`; Copy's and Remove's defaults; Shuffle's `in2`/`out2` sources |
| F15 | Clamp, Invert, Multiply, Add, Gamma, Exposure | Clamp: below `minimum` becomes `MinClampTo` when enabled; Gamma: `v^(1/value)` for positive values, value 0 passes; Exposure: `2^stops`, alpha untouched | Default `channels` of each; Gamma's direction and value 0; Exposure's Densities and Cineon units |
