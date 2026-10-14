# ADR 0002: Nuke's image model — y-up, bbox ≠ format, edge repeat

**Status:** accepted (M0) · **Date:** 2026-10-14

## Context

Artists moving from Nuke rely on its image conventions: coordinates measured up from the
bottom-left, a data window (bbox) that can be smaller or larger than the format, pixels outside the
bbox repeating the edge (which is why "black outside" exists), linear-light premultiplied float, and
arbitrary channels. Changing any of these would change what every knob value means.

## Decision

- Pixel `(x, y)` covers `[x, x+1) × [y, y+1)`, origin bottom-left, centres at `+0.5`.
- An `Image` is a set of named float planes over an integer bbox, rows bottom-up. The format is
  separate (`ImageInfo.format`).
- Reading outside the bbox repeats the nearest edge pixel; nodes that want black there add a one-pixel
  black border. The renderer fetches `region.clamp_into(bbox)`, which answers every read in the region
  under this rule.
- Pixels are 32-bit float, linear light, premultiplied; colour conversion happens only in Read/Write
  (and later colour nodes and the Viewer).

## Consequences

- Knob values (translate, Ramp points, Crop boxes) mean what they mean in Nuke, so `.nk` import can
  copy them across.
- PNG and EXR rows are flipped at the file boundary.
- What the Viewer and Write show outside the bbox (black for now) is listed for verification (F4).
