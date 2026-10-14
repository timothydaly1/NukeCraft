# Parity with Nuke

> **Last reviewed:** 2026-10-14 · **Change:** first version (M0) · **Target:** Foundry Nuke (current release)

How far NukeCraft is from doing what Nuke does, by area. Percentages are judgements against the
checklists in [plan/nuke/node-catalog.md](../plan/nuke/node-catalog.md) and
[plan/execution-plan.md](../plan/execution-plan.md), not measurements; nothing has been compared
with Nuke yet ([fidelity/README.md](fidelity/README.md)).

| Area | ≈ % | Evidence |
|---|---:|---|
| Node library (toolbar breadth) | 5% | 13 classes, several partial (see the catalog) |
| Engine model (bbox/format, channels, ROI, edge repeat, linear premultiplied float) | 60% | In place and tested; missing tiles, threads, proxy, multiview, metadata, deep |
| Animation | 25% | Keys, four interpolations, keying animated knobs; no Curve Editor, tangents, expressions, extrapolation modes |
| Script files | 15% | Own `.ncs` format with repairs; no `.nk` |
| Image formats | 15% | PNG (8/16) and EXR (half/float, data windows, layers on read) |
| Colour management | 10% | sRGB, Rec. 709, gamma, Cineon; no OCIO |
| User interface | 0% | Milestone M1 |
| Scripting and expressions | 0% | Milestone M3 |
| Performance | unknown | Not measured; single-threaded |
| Agent control (beyond Nuke) | ahead | Every command over MCP and CLI; `render_image` for agents |

**Ready for real work: ≈ 0%.** The first useful audience is agents and pipeline tools generating
simple comps headlessly; artists arrive with M1 and M2.

## How these numbers will be measured

As in the Crafting Apps: a weighted feature checklist per area (the node catalog for nodes), oracle
comparisons against Nuke for behaviour (`docs/fidelity/`), and an "essentials user" scenario list (open
plates, key, roto, track, grade, merge, write) walked end to end in the app before each milestone.
