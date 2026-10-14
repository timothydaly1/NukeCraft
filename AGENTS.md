# NukeCraft — rules for agents and contributors

These rules apply to every human and AI contributor. `CLAUDE.md` holds the working instructions;
this file holds the rules that must never be broken. When the two disagree, this file wins.

NukeCraft follows the conventions of the ArtCraft Crafting Apps (PhotoCraft, EffectCraft,
FilmCraft…): pure Rust, clean-room, layered crates, everything a command, everything
agent-drivable, never crash. It is an independent project, not one of those apps.

## Never crash

People trust a compositor with days of work. A malformed script, image file, knob value, expression,
command-line or MCP argument, a corrupt preferences file or a full disk must produce an error the
user (or agent) can act on, never a crash and never lost work. **This rule outranks feature work:**
don't ship a feature by adding a panic path, and fix a crash before building on top of it.

- **Fail with `Result<T, E>`** through the crate's error type and propagate with `?`; add context
  (`map_err`, an error variant) rather than discarding it. Lenient readers (script files) repair what
  they can and report the repairs.
- **No panicking shortcuts in non-test code:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`,
  `todo!` or `unimplemented!`. Use `?`, `ok_or(..)?`, `let … else`, `if let`, or `unwrap_or*` where a
  fallback is genuinely correct (never where it would silently corrupt a script). Unfinished features
  return an "unsupported" error.
- **No `unsafe`** (`unsafe_code = "forbid"` for the workspace).
- **Every input-derived number is hostile.** Use `get()` instead of `[i]`/`[a..b]` when the position
  comes from a file, a user, an agent or arithmetic on those; use checked or saturating arithmetic for
  sizes, offsets and counts; guard division by zero and NaN/infinity; cap allocations sized by input
  (`MAX_PIXELS`, `MAX_NODES`, `MAX_KEYS`, `MAX_RANGE_FRAMES`…).
- **Bound recursion:** node graphs can be cyclic (in a hostile file) or very deep; walks use seen-sets
  and `MAX_DEPTH`, and renders run on a thread with `RENDER_STACK` bytes of stack.
- **Don't cascade:** handle lock poisoning and treat worker-thread join results as `Result`s.
- **Last-resort guard:** `Session::execute` wraps every command in `catch_unwind` and restores the
  script if anything escapes. It is a safety net, not a substitute for the rules above; keep
  `panic = "unwind"`.
- **Prove it:** every crash fix comes with a small synthetic regression test that panicked before the fix.

Tests and `xtask` may `unwrap()`; `clippy.toml` allows it in tests. Every crate carries
`#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`;
new crates start with it.

## 1. Assets: no Foundry artwork, every asset licensed and attributed

This rule is absolute. Breaking it is the most serious mistake a contributor can make on this project.

**What counts as an asset:** any image, icon, cursor, logo, illustration, screenshot, font, colour LUT
or OCIO config, preset, gizmo, toolset, template, sample script, sound, video, 3D model or other
non-code media, in the repository or embedded in code (`include_bytes!`, base64, hard-coded point
lists that reproduce someone else's image).

1. **Never use Foundry iconography, images or other Foundry assets**, in any form: no icons, cursors,
   logos, splash screens, UI artwork or screenshots from Nuke, NukeX, Nuke Studio, Hiero or any other
   Foundry product; no Foundry fonts, LUTs, OCIO configs shipped with Nuke, gizmos, toolsets, sample
   scripts or footage; no traced, redrawn, recoloured or "inspired-by" copies of their icons. Our icons
   may follow generic conventions (a play triangle, an eye, a node), drawn from scratch.
2. **Every asset must be open**: MIT, Apache-2.0, BSD, ISC, zlib, SIL OFL, public domain / CC0, or
   Creative Commons BY / BY-SA (no NC/ND), **or** made by a contributor who licenses it to the project
   under MIT OR Apache-2.0. Unknown or unclear licence: it does not go in.
3. **Every asset needs an attribution sidecar** `X.attribution` next to it:
   ```text
   asset:        <file name>
   title:        <what it is>
   author:       <creator / copyright holder>
   source:       <URL, or "original work">
   license:      <SPDX id or licence name>
   license-file: <path to the licence text, if the licence requires shipping it>
   added:        <YYYY-MM-DD> by <contributor>
   notes:        <how it was made or modified; for screenshots, what is shown>
   ```
   and a row in [`ATTRIBUTION.md`](ATTRIBUTION.md). `cargo xtask assets` (part of `cargo xtask ci`)
   fails if any asset lacks either.
4. **Screenshots** in the repository show only NukeCraft (or other open projects) with media we made or
   that is openly licensed. Never commit screenshots or renders from Nuke.
5. **Local reference material stays local.** Nuke reference captures and notes live only in
   `plan/nuke/ref/` (gitignored). They are never committed, bundled, embedded, traced or shipped.
6. **Code-drawn and procedural content** (icons drawn in code, the demo script, generators) is
   original work under the project licence and is listed in `ATTRIBUTION.md`.
7. **When in doubt, leave it out** and draw or generate it yourself.

## 2. Clean-room code

- Work from Nuke's **public documentation** (the user guide and reference guide, published Python
  and NDK API documentation describe *behaviour*) and from **observing the application**. Never read,
  disassemble or copy anything inside a Nuke installation: binaries, plug-ins, Python modules, gizmos,
  `.nk` toolsets or the NDK example sources. File names and listings only.
- **Never copy GPL/LGPL/AGPL code.** In particular **Natron** (GPL-2.0), **Blender**'s compositor
  (GPL), **G'MIC**, **FFmpeg** and **frei0r** are off limits as sources, even to read while implementing
  the same feature.
- Implement file formats from public specifications (OpenEXR technical documentation, ISO/IEC 15948
  PNG, SMPTE 268 DPX, Kodak Cineon, the OpenFX API specification, the OpenColorIO documentation).
  Don't read the source of reference implementations while implementing a format, even permissively
  licensed ones; spec text and conformance images only. Record the spec edition in the crate README.
- Nuke's own `.nk` script format is read from **scripts we wrote ourselves** in Nuke and its public
  documentation, never from Foundry-supplied scripts or toolsets.
- Dependencies use permissive licences: MIT/Apache-2.0/BSD/ISC/Zlib/Unicode/CC0/BSL-1.0, or MPL-2.0
  used unmodified.

## 3. Engineering rules

See `CLAUDE.md`: pure Rust, dependency layering (`cargo xtask layers`), exact frame time, the image
model (y-up, bbox ≠ format, edge repeat outside the bbox, linear premultiplied float), everything is a
command, everything is agent-drivable, and the quality gate (`cargo xtask ci`) before every commit.

Never commit large binary fixtures. Real-file test corpora are fetched pinned by commit and
sha256-verified, the way the Crafting Apps do it.

## See also

- [ROADMAP.md](ROADMAP.md): stage, headline numbers and what's next
- [docs/gaps.md](docs/gaps.md): every known shortfall against Nuke, ranked; read it before choosing work
- [docs/target-app-parity.md](docs/target-app-parity.md): how far along each area is, and how that was judged
- [CONTRIBUTING.md](CONTRIBUTING.md): setup, gates, commits, how to add things
- [docs/architecture.md](docs/architecture.md): layers, image model, scripts, rendering, commands
- [docs/agents.md](docs/agents.md): driving NukeCraft over MCP and the command line
- [docs/script-format.md](docs/script-format.md): the `.ncs` script file
- [docs/testing.md](docs/testing.md) and [docs/fidelity/README.md](docs/fidelity/README.md): tests, and the behaviours still to be checked against Nuke
- [ATTRIBUTION.md](ATTRIBUTION.md): asset index
