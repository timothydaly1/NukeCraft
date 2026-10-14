# Node catalog

Nuke's toolbar nodes by category, from Foundry's public reference guide and observing the menus, with
NukeCraft's status. Class names are given where Nuke's class differs from the menu name. Third-party
keyers bundled with Nuke (Keylight, Primatte, Ultimatte) are other companies' products; NukeCraft would
implement its own keyers rather than clones of those.

Status: ✅ implemented (pending fidelity checks) · ◐ partial · — not started. Milestone in brackets.

## Image

| Node | Status | Notes |
|---|---|---|
| Read | ◐ | PNG, EXR; no frame range/before/after, proxy, DPX/TIFF/video [M9] |
| Write | ◐ | PNG 8/16, EXR half/float; no views, metadata, render order [M9] |
| Constant | ✅ | |
| CheckerBoard (CheckerBoard2) | ◐ | Two colours; no color2/3, lines [M2] |
| ColorBars, ColorWheel | — | [M2] |
| Viewer | — | App feature [M1] |

## Draw

| Node | Status | Notes |
|---|---|---|
| Ramp | ◐ | No input/bbox options [M2] |
| Rectangle, Radial | — | [M2] |
| Roto, RotoPaint | — | [M2] (paint later) |
| Noise, Text, Grain, Dither, Flare, Glint, LightWrap | — | |

## Time

| Node | Status | Notes |
|---|---|---|
| FrameHold, TimeOffset, FrameRange | — | [M2] |
| Retime, TimeWarp, FrameBlend, TimeEcho, AppendClip | — | |
| Kronos, OFlow (optical flow retiming) | — | |

## Channel

| Node | Status | Notes |
|---|---|---|
| Shuffle | ◐ | Classic Shuffle (`in`, `out`, red/green/blue/alpha from a channel, black or white); no `in2`/`out2` |
| Shuffle2, ShuffleCopy | — | Shuffle2's mapping knob with `.nk` import [M3] |
| Copy | ✅ | Four channel pairs, layer copy, bbox, mask, mix |
| ChannelMerge | ✅ | Twelve operations (F16) |
| Remove | ✅ | Remove or keep up to four channel sets |
| AddChannels | — | [M2] |

## Color

| Node | Status | Notes |
|---|---|---|
| Grade | ✅ | Mask input, unpremult, mix (as every colour node) |
| Saturation | ✅ | Four luminance formulas |
| ColorCorrect | ◐ | Master + shadows/midtones/highlights; fixed range curves (no `lookup` editing), contrast pivot unverified (F13) |
| HueCorrect, HueShift | — | [M2] (needs the Curve Editor) |
| Clamp, Invert, Multiply, Add, Gamma | ✅ | |
| Exposure (EXPTool) | ◐ | Stops only; no Densities/Cineon (F15) |
| ColorLookup, ColorMatrix, Colorspace, Log2Lin | — | [M5] |
| OCIOColorSpace, OCIODisplay, OCIOFileTransform, OCIOCDLTransform | — | [M5] |
| HSVTool, Posterize, Histogram, MinColor, Toe, Expression | — | |

## Filter

| Node | Status | Notes |
|---|---|---|
| Blur | ✅ | Box, triangle, quadratic, gaussian; size mapping unverified (F3) |
| Defocus, ZDefocus, Convolve | — | [M2]/later |
| Sharpen, Soften, EdgeDetect, Emboss, Median | — | [M2] |
| Erode (FilterErode), Dilate, EdgeBlur | — | [M2] |
| Glow, Bloom | — | [M2] |
| DirBlur, MotionBlur, VectorBlur | — | [M6] |
| Denoise, Bilateral | — | |

## Keyer

| Node | Status | Notes |
|---|---|---|
| Keyer, HueKeyer, ChromaKeyer, Difference | — | [M7] |
| IBKColour, IBKGizmo | — | [M7] |
| Cryptomatte | — | [M7] |

## Merge

| Node | Status | Notes |
|---|---|---|
| Merge (Merge2) | ✅ | 29 operations (soft-light not yet); mask with invert; no A/B channel selection, also-merge, alpha masking |
| Premult, Unpremult | ✅ | |
| Switch | ✅ | |
| Keymix, Dissolve, AddMix | — | [M2] |
| ContactSheet, CopyRectangle, CopyBBox, MergeExpression, ZMerge | — | |

## Transform

| Node | Status | Notes |
|---|---|---|
| Transform | ✅ | Impulse + six cubics; no motion blur, Notch/Lanczos/Sinc, on-viewer handle [M1/M6] |
| Crop | ✅ | No softness |
| Reformat | — | [M2] |
| CornerPin2D, Mirror, Position, Tile | — | [M2] |
| Tracker, PlanarTracker, CameraTracker | — | [M6] |
| STMap, IDistort, SplineWarp, GridWarp, LensDistortion | — | [M6] |
| AdjustBBox, BlackOutside, TransformMasked, SphericalTransform | — | |

## 3D, Particles, Deep, Views, Metadata

| Area | Status | Milestone |
|---|---|---|
| 3D: Camera, Axis, Card, Sphere, Cube, Scene, ScanlineRender, ReadGeo, Light | — | M8 |
| Particles | — | later |
| Deep: DeepRead, DeepMerge, DeepToImage, DeepHoldout, DeepRecolor | — | M8 |
| Views: JoinViews, OneView, SideBySide, Anaglyph | — | M8 |
| Metadata: ViewMetadata, ModifyMetaData, CopyMetaData | — | M8 |

## Other

| Node | Status | Notes |
|---|---|---|
| Dot | ✅ | |
| NoOp, Backdrop, StickyNote | — | [M2] |
| Group, Input, Output, Precomp, LiveGroup | — | [M2]/[M10] |
| BlinkScript-style kernels | — | [M10] (WGSL or WebAssembly) |
