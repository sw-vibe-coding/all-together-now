## Step 1: Tiled Foundation

Replace the heat-driven treemap layout with a simple tiled grid.
Coord top-left, workers in name order, every tile the same size.
No auto-refresh, no zoom oscillation, no ordering churn.

### Deliverables

1. Remove / neutralize the scale-UI machinery:
   - `squarify`, `groupedSquarify`, `groupItems`, `worstRatio`, `layoutRow`
     delete (or keep archived — unused).
   - `computeLayout` rewritten to emit a simple tiled grid.
   - `buildLayoutItems` emits agents sorted by name, coord first.
   - `matchesFilter` stays but is a no-op for this step (no filter UI).
2. New state in `layoutState`:
   - `layoutMode: 'tiled'` (only supported value this step)
   - `sortMode: 'name'`
3. `computeLayout` places:
   - Coord agent (first one with role === 'coordinator' in name order)
     fills a cell in the top-left that's 2x the size of worker cells.
   - Workers fill the remaining grid in name order, equal-sized.
   - Grid dimensions adapt to the current window aspect ratio.
4. Each panel gets `position: absolute; left/top/width/height` from
   the layout. The existing `.agent-panel` CSS + controls stay
   unchanged — no new chrome yet (that's step 2).
5. The ↻ Refresh button stays (useful for manual recompute) but
   the event listener is simplified to just call applyLayout. No
   hysteresis checks, no heat diff threshold.
6. Keep the fixed-native-xterm + CSS scale from step 3 of scale-UI
   (still the right answer for tile sizes that don't match native
   geometry). Wipe the oscillation-prone hysteresis — with tile
   sizes now stable, a single `Math.min(tileW/natW, tileH/natH)`
   snapped to the nearest TILE_SCALES entry is enough.
7. localStorage persists `layoutMode` + `sortMode` under a new
   `atn-window-ui-v1` key (separate from scale-UI's
   `atn-scale-ui-v1`, which stays for backward compat but is
   ignored by the new code).

### Acceptance

- 4-agent local demo (1 coord + 3 workers) shows: coord in the
  top-left at 2x worker size; workers-1/2/3 tiled in name order
  at equal size. No zoom oscillation on Refresh.
- Window resize re-lays-out tiles to fit the new viewport.
- Agent add/delete triggers a re-layout automatically.
- `cargo test --workspace` green; `cargo clippy --workspace
  --all-targets -- -D warnings` clean.
