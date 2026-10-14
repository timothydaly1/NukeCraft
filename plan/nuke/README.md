# Nuke reference: how to observe it cleanly

NukeCraft is written from Nuke's **public documentation** and from **observing the application**.
This folder holds the catalogues derived from that; `plan/nuke/ref/` (gitignored) holds local
reference material that must never be committed.

## Allowed

- Reading Foundry's public user guide, reference guide, release notes, and Python/NDK API docs, for
  behaviour (what a knob does, what an operation computes).
- Using a licensed copy of Nuke, within its licence terms, to observe behaviour: building **our own**
  synthetic scripts and rendering them, interactively or headless (`nuke -t script.py`), and recording
  the numbers.
- Committing those **numbers** (sample values from our own synthetic inputs) under `docs/fidelity/`.

## Not allowed

- Reading, disassembling or copying anything inside a Nuke installation (binaries, plug-ins, Python
  modules, gizmos, toolsets, NDK example sources).
- Committing Nuke screenshots, renders, `.nk` files written by Nuke, gizmos or toolsets.
- Reading Natron or any other GPL/LGPL/AGPL compositor's source.

## Measuring

The fidelity harness generates the Nuke-side script: `docs/fidelity/README.md` → "Measuring with
Nuke". Keep the generated script and Nuke's output here (`plan/nuke/ref/`) until the numbers are
copied to `docs/fidelity/oracle.json`.

## Files here

- [node-catalog.md](node-catalog.md): Nuke's toolbar nodes by category, with NukeCraft's status.
