# Driving NukeCraft from agents

Everything NukeCraft can do is an engine command with an id and JSON parameters. Agents reach every
command three ways, all going through `Session::execute` (so every edit is validated, undoable and
atomic).

## MCP server

`nukecraft-cli mcp [SCRIPT.ncs | --demo]` speaks the Model Context Protocol over stdio (JSON-RPC 2.0,
one message per line). This repository ships a ready [`.mcp.json`](../.mcp.json) that starts it with
the demo script.

| Tool | Arguments | Returns |
|---|---|---|
| `execute` | `{command, params}` | the command's JSON result, or an error message (`isError: true`) |
| `list_commands` | `{}` | every command: id, label, menu, shortcut, parameters |
| `node_classes` | `{}` | every node class: category, inputs, knobs (kind, default, choices) |
| `get_script` | `{}` | the open script as `.ncs` JSON |
| `render_image` | `{node, frame?, max_size?}` | the node's output over its format as a PNG (sRGB), plus a caption with format, bbox and channels |

A typical loop: `node_classes` once, then `execute` `node.create` / `node.connect` / `node.setKnob`,
`render_image` to look, `execute` `edit.undo` when something is wrong, `file.save` at the end.

## Command line

```sh
nukecraft-cli demo demo.ncs
nukecraft-cli exec node.create '{"class":"Grade","inputs":["Merge2"],"knobs":{"white":1.4}}' demo.ncs --save
nukecraft-cli exec node.setKnob '{"node":"Transform1","knob":"rotate","value":45,"frame":12}' demo.ncs --save
nukecraft-cli info demo.ncs --node Grade1 --frame 12
nukecraft-cli frame demo.ncs --node Grade1 --frame 12 --out grade.png
nukecraft-cli render demo.ncs --frames 1-24
nukecraft-cli commands
nukecraft-cli nodes
```

Results are JSON on stdout; errors go to stderr with exit code 1 (2 for usage errors).

## Things worth knowing

- **Coordinates** are pixels with y **up** from the bottom-left corner of the format.
- **Merge inputs:** 0 is B (the background), 1 is A (the foreground), 2 is the mask. `A over B` is the
  default operation.
- **Knob values** are a number, a list of numbers (`[x, y]`, `[r, g, b, a]`), `true`/`false` or text
  (choices, channels, files, formats). One number for a colour or XY knob applies to every component.
- **Animation:** `node.setKnob` with `frame` sets a key; editing an animated knob without `frame` keys
  the current frame (`time.goto`), as in Nuke.
- **Names:** Nuke's class and knob names (`Merge2`, `CheckerBoard2`, `black_clamp`); menu names work
  too (`Merge`, `CheckerBoard`).

## Not yet

A control channel into a running desktop app (with screenshots and widget automation ids) arrives
with the app in M1, bridged into the MCP server as EffectCraft's `--bridge` is.
