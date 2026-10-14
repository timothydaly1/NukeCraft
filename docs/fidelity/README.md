# Fidelity: behaviours not yet checked against Nuke

> **Last updated:** 2026-10-14 · **Measured cases:** 0

Every entry is an implementation choice made from public documentation and general knowledge, not
yet compared with Nuke itself. Each needs an oracle case ([testing.md](../testing.md#fidelity-against-nuke))
before it can be called matching. Ordered by how often the behaviour shows up in real scripts.

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
| F9 | CheckerBoard | Two colours, boxes measured from the format centre | Four colours, line colours and widths, the box phase |
| F10 | Ramp | `smooth0`/`smooth1`/`smooth` as cubic ease curves; alpha ramps with the colour | The exact curves |
| F11 | Read colour | PNG defaults to sRGB, EXR to linear; `premultiplied` divides by alpha around the conversion | Default colour spaces per file type in the current Nuke colour management |
| F12 | Blur `crop`, Crop `black outside` | One-pixel black border added outside the box | Border handling with fractional boxes and `softness` |
