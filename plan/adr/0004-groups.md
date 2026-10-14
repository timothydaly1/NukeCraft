# ADR 0004: Groups as parent links, nodes named by path

**Status:** accepted (M2.12) · **Date:** 2026-10-14

## Context

Nuke's Groups (and gizmos, which are Groups saved to a file) hold their own nodes: names are unique
within a group, `Input` nodes are the group's inputs and an `Output` node its output, and scripts
and expressions refer to nested nodes by path (`Group1.Blur1`). NukeCraft's script was a flat list
of nodes addressed by unique names, and every command, the CLI, MCP and the UI name nodes that way.

Two shapes were possible: a nested script inside each Group node, or a flat list where each node
records the Group it is in.

## Decision

- A node has an optional `parent`: the Group it is inside. The script stays one flat list with one
  id space; groups are a view over it.
- Names are unique among the nodes of one group. Commands name nodes by **path**: a top-level name
  (`Blur1`, unchanged) or names joined by dots through groups (`Group1.Blur1`). Node names can't
  contain dots, so paths are unambiguous. `node.create` takes `group`; inputs given by name there
  are siblings.
- Pipes connect nodes of the same group only. A Group's inputs are its `Input` nodes (by their
  `number` knob), its output is its `Output` node. The renderer reads through them: a Group
  stands for its Output, an Input for the Group's input `number`; nothing is copied.
- `node.group` collapses nodes into a new Group (an Input per outside source, an Output for the
  node used outside); `node.ungroup` expands one, keeping the pipes. Copy and paste carry a
  Group's contents.
- The script schema becomes 2 (files with groups would be misread by schema-1 readers). Loading
  repairs missing parents, loops and duplicate names within a group.

## Consequences

- Every existing command, the CLI and MCP work inside groups unchanged, through paths; agents can
  address any node without knowing which group the UI shows.
- `.nk` import (M3) maps Nuke's `Group { … }` / `end_group` blocks to parent links directly, and
  gizmos to Groups.
- Expressions (M3) resolve names relative to the group they are written in, as Nuke's do.
- Group-level user knobs (exposed knobs on the Group) are still to come, with user knobs in M3.
