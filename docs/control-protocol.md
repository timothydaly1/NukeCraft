# Control protocol

`nukecraft --control PORT` opens the desktop app with a control channel on `127.0.0.1:PORT`:
JSON lines over TCP, one request per line, one reply per line. Requests are handled on the UI
thread at the start of the next frame, so they see and change exactly what the person at the
window sees. `nukecraft-cli mcp --bridge PORT` puts an MCP server in front of it.

```text
→ {"id": 1, "method": "engine.execute", "params": {"command": "node.create", "params": {"class": "Blur"}}}
← {"id": 1, "result": {"name": "Blur1", "class": "Blur"}}
→ {"id": 2, "method": "ui.click", "params": {"id": "graph.node.Blur1", "clicks": 2}}
← {"id": 2, "result": {}}
→ {"id": 3, "method": "nope"}
← {"id": 3, "error": "unknown method `nope` (…)"}
```

## Methods

| Method | Params | Result |
|---|---|---|
| `engine.execute` | `{command, params}` | The command's result (any engine command; see `query.commands`) |
| `app.state` | `{}` | `{viewed, selection, frame, revision, dirty, path, busy, viewerError, render}` (`render`: `{done, total, current}` while Render… runs in the background, else null) |
| `app.view` | `{node}` | Views a node in the Viewer (what pressing 1 does) |
| `ui.inspect` | `{prefix?}` | The on-screen elements whose automation id starts with `prefix`: `[{id, label, rect: [x, y, w, h]}]` |
| `ui.click` | `{id, clicks?: 1‥3, modifiers?: ["shift", "alt", "cmd"]}` | Clicks the centre of an element (queued: it happens over the next frames) |
| `ui.key` | `{key, modifiers?}` | Presses and releases a key (egui key names: `B`, `Tab`, `Delete`, `ArrowLeft`…) |
| `ui.text` | `{text}` | Types text into the focused field |
| `ui.screenshot` | `{path}` | Writes a PNG of the window (answered once the window has drawn it) |

## Automation ids

Every interactive element registers a stable id each frame. The main families:

| Id | Element |
|---|---|
| `menu.<Menu>`, `menu.<Menu>.<command id>`, `menu.Nodes.<Class>` | Menus and their items |
| `graph.canvas` | The Node Graph |
| `graph.node.<Name>`, `.output`, `.input.<i>` | A node, its output, the grab handle of input `i` |
| `graph.search`, `graph.search.item.<Class>` | The Tab search field and its results |
| `viewer.image`, `viewer.channel`, `viewer.exposure`, `viewer.gamma`, `viewer.fit` | The Viewer and its toolbar |
| `viewer.first`, `.prev`, `.play`, `.next`, `.last`, `.frame`, `.slider` | The timeline |
| `props.<Name>.<knob>` (`.1`, `.2`… for further components), `.key`, `.browse` | Knob widgets and their key buttons |
| `props.<Name>.name`, `.disable`, `.close`, `.render` | A node's panel header (and Write's Render button) |
| `dialog.unsaved.save` / `.dontsave` / `.cancel`, `dialog.settings.apply` | Dialogs |

## Headless

The same UI runs without a window: `cargo run -p nukecraft-ui-egui --example snapshot -- --out ui.png
--step '<request>'…` accepts `engine.execute`, `ui.click`, `ui.drag {from, to}`, `ui.key`,
`ui.text`, `ui.hover`, `ui.inspect` and `snap {path}` steps and writes a PNG rasterised on the CPU.
It needs no display or GPU, so it works in CI and cloud containers.
