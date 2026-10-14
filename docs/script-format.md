# The `.ncs` script file

A NukeCraft script is UTF-8 JSON, written pretty-printed so it diffs cleanly in version control.

```json
{
  "savedBy": "NukeCraft 0.1.0",
  "schema": 1,
  "root": {
    "format": { "width": 640, "height": 360, "pixel_aspect": 1.0 },
    "fps": { "num": 24, "den": 1 },
    "frames": { "first": 1, "last": 24 }
  },
  "nodes": [
    { "id": 1, "name": "CheckerBoard1", "class": "CheckerBoard2", "knobs": { "boxsize": 32.0 }, "x": 0.0, "y": 0.0 },
    {
      "id": 2, "name": "Transform1", "class": "Transform", "inputs": [1], "x": 0.0, "y": 80.0,
      "knobs": {
        "center": [128.0, 128.0],
        "rotate": { "curve": [ { "t": 1, "v": 0.0, "i": "linear" }, { "t": 24, "v": 90.0, "i": "linear" } ] }
      }
    }
  ]
}
```

## Fields

- `savedBy`: the version that wrote the file (informational).
- `schema`: bumped when a stored value changes meaning. A file with a newer schema is refused rather
  than misread; older schemas are converted on open.
- `root`: the format (`width`, `height`, `pixel_aspect`, optional `name`), the frame rate as a ratio,
  and the frame range.
- `nodes`, in creation order:
  - `id`: unique within the file; `inputs` refer to ids, `null` for an empty input.
  - `name`: unique; letters, digits and `_`, not starting with a digit.
  - `class`: Nuke's class name.
  - `knobs`: only knobs that differ from their default. A value is a number, a list of numbers,
    `true`/`false`, text, or `{"curve": [keys]}` (per component inside a list). A key is
    `{"t": frame, "v": value, "i": interpolation}`; `i` is `constant`, `linear`, `horizontal` or
    `catmull-rom` (the default, omitted).
  - `x`, `y`: position in the Node Graph; `disabled`, `label` when set.

## Loading is lenient

What can be repaired is repaired and reported (`file.open` returns the warnings): duplicate ids get
new ones, invalid or duplicate names are renamed, inputs pointing at missing nodes or closing a
cycle are disconnected, invalid knob values revert to their defaults, non-finite positions become 0.
Malformed JSON and numbers JSON can't hold are errors.

## Relation to Nuke's `.nk`

`.nk` is Nuke's own TCL-flavoured text format. NukeCraft keeps its own format (decided in
[plan/adr/0003-script-format.md](../plan/adr/0003-script-format.md)) and will import and export `.nk`
as interchange (milestone M3). Class, knob and input conventions already follow Nuke's so that the
mapping is direct.
