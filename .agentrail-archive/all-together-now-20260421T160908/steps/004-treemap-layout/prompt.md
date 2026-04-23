## Step 4: Treemap Layout

Replace the fixed cols-1..cols-4 grid in the dashboard with a squarified
treemap whose tile areas are proportional to the heat score from step 1.
A single focus panel occupies a large portion of the viewport (default
~40-50%); the rest of the screen is the treemap.

### Deliverables

1. Layout state at the top of `init()` in the dashboard:
   - `layout.focusId` (string | null)
   - `layout.tiles` (ordered list derived from /api/agents + /api/agents/heat)
   - `layout.freezeUntil` (timestamp — pause resize until after a user
     interaction)
2. Squarified treemap implementation in plain JS (≈80 lines). Input:
   remaining-rect + list of `{id, heat}`. Output: absolutely-positioned
   `{x, y, w, h}` per tile. No heavy deps.
3. Tile chooser: for each tile's `{w, h}`, compute the CSS scale from
   step 3; if below threshold, render compact tile instead of xterm.
4. Heat smoothing: ingest raw `heat` via EWMA over ~5s so tiny byte
   bursts don't cause visible tile jitter.
5. Refresh cadence 1-2s. Suspend refreshes for 5s after any click-to-focus
   so the user isn't fighting the layout.
6. Auto-pick focus: if `layout.focusId` is null, default to highest
   heat agent with role=coordinator, else highest heat overall.
7. Keep the existing view-tabs/Graph/Saga/Wiki/Events surfaces unchanged
   (treemap is only inside the Agents tab).

### Acceptance

- With 10+ agents, the focus panel is large and the treemap tiles scale
  with their relative heat. Typing produces visible tile size shifts on
  the smoothed cadence.
- Tile sizes never drop below the compact-tile threshold without switching
  to compact mode.
- Clicking a treemap tile swaps it to focus; the freeze timer prevents
  immediate re-layout.
- Browser devtools: only 1-2 network calls per second (heat poll); no
  per-byte DOM thrash.
