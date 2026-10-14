# Status

> **Updated:** 2026-10-14

- **Milestone:** M1 — the desktop app. M1.1 (app shell) and M1.2 (Node Graph) done.
- **Next task:** M1.3 — Viewer with a background render thread (`crates/ui-egui/src/viewer.rs`).
- **Gate:** `cargo xtask ci` green on `main` (91 tests).
- **Looking at the UI:** `cargo run -p nukecraft-ui-egui --example snapshot -- --out ui.png` (headless,
  no GPU). The real window also runs under Xvfb with Mesa's software GL.
- **Blockers:** none. Open decision for the maintainer: the project name (see README's trademark note).
- **Running agents:** none.

## Recently done

- M1.2: Node Graph: category colours, pipes with input labels, pan/zoom/frame, select, box
  select, drag to move (one undo step), drag pipes to connect/reconnect/disconnect, Tab search,
  node keys that insert (Shift: branch) and make room, 1 to view, Backspace/Delete, context menu.
  Deleting a chain now reconnects its ends.

- M1.1: `nukecraft-ui-egui` (L5) and the `nukecraft` app (L6): menus from the command registry,
  shortcuts, file dialogs, Project Settings, unsaved-changes prompt, autosave, undo groups in the
  engine, automation registry, headless snapshots with a CPU rasteriser.
- M0.1–M0.6: workspace and rules; L0–L6 crates; 13 node classes; PNG/EXR; CLI; MCP server; demo.
