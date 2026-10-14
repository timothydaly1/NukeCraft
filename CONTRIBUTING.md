# Contributing to NukeCraft

Thanks for helping. Please read [AGENTS.md](AGENTS.md) first: its rules on assets and clean-room
work apply to everyone, people and AI agents alike, and they are not negotiable.

## Setup

You need [Rust](https://rustup.rs/) 1.95 or newer, and the wasm target for the full gate:

```sh
rustup target add wasm32-unknown-unknown
cargo test --workspace
cargo run -p nukecraft-cli -- demo demo.ncs
cargo run -p nukecraft-cli -- frame demo.ncs --node Write1 --frame 12 --out frame12.png
cargo xtask ci                           # what every commit must pass
```

## Ground rules

- **Clean room.** Work from Nuke's public documentation and its observable behaviour only. Never use
  Foundry artwork, gizmos, toolsets or code, and never copy GPL/LGPL/AGPL code (Natron included).
- **Every asset is attributed.** Add a `.attribution` sidecar and an [ATTRIBUTION.md](ATTRIBUTION.md)
  row; `cargo xtask assets` checks it.
- **Layering.** Dependencies point down the layers in [docs/architecture.md](docs/architecture.md);
  nothing below the interface layer depends on UI or OS crates.
- **Everything is a command.** New features are engine commands with an id, label, menu path,
  shortcut and parameters, so the UI, the CLI and the MCP server get them at once.
- **Never crash.** Non-test code returns errors instead of panicking (no `unwrap()`, `expect()`,
  `panic!`, `unreachable!`, `todo!`, `unimplemented!` or `unsafe`), uses checked indexing and arithmetic
  on input-derived values, and every crash fix brings a regression test.
- **Tests with every change**, and a green `cargo xtask ci` before every commit. One task per commit.

## How to add…

- **A node:** add a `NodeSpec` in `crates/nodes` (Nuke's class name, menu label and category, its
  inputs in Nuke's order, its knobs with Nuke's names, and `info`, `request` and `engine`
  functions). Test the image it makes, its bbox, an unconnected input and hostile knob values. List
  anything you could not check against Nuke in [docs/fidelity/README.md](docs/fidelity/README.md) and
  update [plan/nuke/node-catalog.md](plan/nuke/node-catalog.md).
- **A command:** add a `CommandSpec` to `crates/engine/src/commands.rs` and test it, including undo and
  a failing call leaving the script unchanged.
- **A file format:** a module in `crates/io` from the public specification, with round-trip tests and
  tests for truncated and hostile files.

## Talk to us

Questions, ideas and bug reports are welcome in GitHub issues.
