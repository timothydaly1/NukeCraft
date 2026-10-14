# ADR 0001: Script time is frames, counted in 720 720 subframes

**Status:** accepted (M0) · **Date:** 2026-10-14

## Context

The Crafting Apps share an exact media time base, `Tick` = 1/254 016 000 000 s, so edits never drift
between frame rates and audio rates. A compositing script is different: Nuke keys knobs on **frame
numbers**, its root holds a frame range, and changing the script's frame rate does not move keys.
Motion blur and retiming still need exact fractions of a frame (shutter samples at 1/3, 1/10…).

## Decision

- Script time is `FrameTime`: an `i64` count of subframes, `SUBFRAMES_PER_FRAME = 720 720`
  (lcm(1‥16)), clamped to ±10⁹ frames.
- Files hold plain frame numbers (`12`, `12.5`); they round-trip exactly because every subframe count
  in range is below 2⁵³.
- `Tick` stays for exchanging media with FilmCraft and EffectCraft: `FrameTime::to_tick(fps)` is exact
  for whole frames at every broadcast rate and rounds subframes to the nearest tick.

## Consequences

- Shutter offsets from 1 to 16 samples over common shutter angles are exact; finer samplings round
  to the nearest 1/720 720 of a frame (≈ 58 ns at 24 fps), well below anything visible.
- Code never compares frame `f64`s for equality; keys are found by integer subframe.
