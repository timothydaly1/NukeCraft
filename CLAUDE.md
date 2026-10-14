# NukeCraft — instructions for agents

NukeCraft is a clean-room, open-source, pure-Rust node-based compositor targeting Foundry Nuke parity
(and beyond). Native on macOS, Windows and Linux; the engine (L0–L4) also builds for WASM. It is built
the way the ArtCraft Crafting Apps (PhotoCraft, EffectCraft, FilmCraft…) are built and follows their
conventions, but it is an independent project.

## Start every session here
Unlike the Crafting Apps, `plan/` is **tracked** in this repository (sessions run in fresh cloud
containers, so a local-only plan would be lost); only `plan/nuke/ref/` is gitignored.
1. Read [`plan/STATUS.md`](plan/STATUS.md): current milestone, next task, blockers.
2. Read the task in [`plan/execution-plan.md`](plan/execution-plan.md), the relevant section of
   [`docs/architecture.md`](docs/architecture.md), and the crate docs you touch. Node classes and
   their status: [`plan/nuke/node-catalog.md`](plan/nuke/node-catalog.md). Decisions: [`plan/adr/`](plan/adr/).
3. Read [`docs/gaps.md`](docs/gaps.md): every known shortfall, ranked. Reliability and fidelity come
   before breadth.
4. Work autonomously through the plan; update `plan/STATUS.md` and the docs your change makes stale
   in the same commit.

## Non-negotiables
**Read [`AGENTS.md`](AGENTS.md) first; its rules override everything here.** No Foundry assets, every
asset licensed and attributed (`cargo xtask assets`).
- **Never crash.** Non-test code has no `unwrap()`/`expect()`/`panic!`/`unreachable!`/`todo!`/
  `unimplemented!` and no `unsafe`: return `Result` and use `?`, `ok_or`, `let … else`. Input-derived
  indices, slices and arithmetic use `get()` and checked/saturating math; graph walks are depth-limited
  and cycle-checked; every crash fix gets a regression test.
- **Clean-room.** Public docs and observed behaviour only. No GPL/LGPL/AGPL code — Natron above all.
- **Pure Rust.** No C/C++ dependencies. Media codecs, when they come, come from FilmCraft's crates as
  pinned git dependencies (as EffectCraft does).
- **Layering** (`cargo xtask layers`): dependencies point down; nothing below L5 depends on
  egui/eframe/winit/rfd/cpal; L0–L4 build for wasm32 (`cargo xtask wasm`).
- **Exact time:** `nukecraft_time::FrameTime` (720 720 subframes per frame). Keys sit on frames, as in
  Nuke; `Tick` (254 016 000 000/s) is only for exchanging media with the sibling apps.
- **Image model:** y-up pixel grid with the origin at the bottom-left, pixel centres at `+0.5`; the
  bbox (data window) is separate from the format (display window) and may extend beyond it; reads
  outside a bbox **repeat the edge pixel**; pixels are linear-light, premultiplied 32-bit float.
- **Nuke's names:** node classes (`Merge2`, `CheckerBoard2`), knob names (`translate`, `black_clamp`),
  input order (Merge: 0 = B, 1 = A, 2 = mask) and menu labels follow Nuke, so `.nk` import can map
  them one to one.
- **Everything is a command** (`crates/engine/src/commands.rs`): id, label, menu path, shortcut, params,
  enabled(), run(). The UI, the CLI and the MCP server dispatch by id; a failing command leaves the
  script unchanged; undo is automatic.
- **Everything is agent-drivable:** new features are commands, new nodes are registry entries, so
  agents get them through MCP without extra work. Every interactive widget (once the UI exists)
  registers an automation id.
- **Quality gate** before every commit: `cargo xtask ci` (fmt, clippy -D warnings, tests, layers,
  assets, wasm). Only green states are committed.
- **Commits:** one task id per commit (`M1.2: node graph panel`).

## Running it
- `cargo run -p nukecraft-cli -- demo demo.ncs` writes the demo script;
  `cargo run -p nukecraft-cli -- frame demo.ncs --node Write1 --frame 12 --out f12.png` renders a frame.
  **Look at the result** (open the PNG) when you change pixels.
- `cargo run -p nukecraft-cli -- exec node.create '{"class":"Blur","inputs":["Grade1"]}' demo.ncs --save`
  runs any engine command against a script.
- `cargo run -p nukecraft-cli -- mcp --demo` serves MCP on stdio ([`.mcp.json`](.mcp.json) wires it up).
- There is no desktop app yet (milestone M1).
- Nuke reference: run your own synthetic scripts in a licensed Nuke (`nuke -t script.py` renders
  headless) and record **numbers** in `docs/fidelity/`; captures stay in `plan/nuke/ref/` (gitignored).
  See [`plan/nuke/README.md`](plan/nuke/README.md).
- Parallel agents: separate git worktrees and `CARGO_TARGET_DIR=target/agent-<name>`.
