# ADR 0003: Own JSON script format; `.nk` as interchange

**Status:** accepted (M0) · **Date:** 2026-10-14

## Context

Nuke's `.nk` is a TCL-flavoured text format with an implicit node stack, inline expressions, user knobs
and version-specific knob forms. Reading it well is a large task (M3). The Crafting Apps keep their own
readable, versioned JSON (`.ecproj`, …) and treat the host app's format as import/export.

## Decision

- NukeCraft's native format is `.ncs`: pretty-printed JSON with `savedBy` and `schema`, only
  non-default knobs, connections by stable id, a lenient loader that repairs and reports.
- `.nk` becomes import and export in M3. Node class names, knob names, knob value shapes and input
  order already follow Nuke's, so the mapping is mostly direct.

## Consequences

- Scripts diff cleanly and are trivial for agents and tools to generate.
- Schema history: 1 (M0); 2 (M2.12) adds each node's `parent` group ([ADR 0004](0004-groups.md)).
- Studio pipelines that read or write `.nk` need the M3 converter; until then NukeCraft can't open
  existing Nuke work.
