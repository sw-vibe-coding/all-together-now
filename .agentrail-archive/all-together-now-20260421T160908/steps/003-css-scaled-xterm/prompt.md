## Step 3: CSS-Scaled xterm

Render each xterm at a fixed "native" size (120x40). Wrap it in a
`.term-scale` div that applies `transform: scale(k); transform-origin: top
left;` to fit the tile's actual pixel dimensions. PTY size is no longer
dragged around by layout churn — only an explicit focus-to-pin triggers
an actual PTY resize.

### Deliverables

1. `.term-scale` wrapper around every `.agent-terminal` div. CSS:
   `transform-origin: top left; transform: scale(var(--term-k, 1));`.
2. Layout code computes `k` per tile from `tile_width_px / native_width_px`
   and `tile_height_px / native_height_px`, takes the min, snaps to the
   nearest entry in a small set (e.g. 0.25, 0.33, 0.5, 0.66, 0.8, 1.0) to
   keep text crisp.
3. Below the smallest scale (0.25 or similar), switch that tile to the
   compact variant from step 2 automatically.
4. Sever the existing per-tile PTY-size sync from window `resize`
   events. Keep PTY-sync triggered only when a tile is promoted to the
   focus panel (single panel; large and crisp).
5. xterm render count doesn't change on layout shuffle — scale updates
   are pure DOM. Document this in a short comment block above the layout
   code.
6. Unit tests for the scale picker (pure JS, plain numbers → snapped
   value).

### Acceptance

- Resizing the browser window only changes the CSS `transform` on each
  tile; no `POST /api/agents/{id}/resize` calls fire in devtools network.
- Clicking a tile into focus does fire a resize that matches the focused
  dimensions.
- Text is crisp at each snapped scale; below threshold tiles visibly
  switch to the compact layout.
- cargo test / clippy still green (no Rust changes expected).
