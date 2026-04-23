## Step 3: Stack Layout

Add the Stack layout mode: one primary window at ~80% viewport +
everything else in the minimized strip.

### Deliverables

1. `layoutMode` supports `'stack'`. Layout selector in the top bar
   with Tiled / Stack buttons.
2. When switching to Stack: auto-minimize every window except the
   selected one (or coord if nothing selected). The selected/coord
   window becomes "primary" at ~80% viewport.
3. Clicking a minimized cell in Stack mode promotes it to primary
   and minimizes the previous primary.
4. Switching back to Tiled restores every non-pinned window to
   `normal` state.
5. localStorage carries `layoutMode` (now picks between tiled/stack).

### Acceptance

- Toggling `Stack` in the top bar moves all but one agent into the
  bottom strip; primary fills ~80% of viewport.
- Clicking any minimized cell swaps it with the primary.
- Toggling back to `Tiled` restores the grid.
- cargo test + clippy clean.
