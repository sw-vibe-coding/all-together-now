## Step 4: Carousel Layout

Add the Carousel layout: focused window ~80% center, peek of
prev/next on either side, cycle through with ◀/▶.

### Deliverables

1. `layoutMode` supports `'carousel'`. Selector button added.
2. Focused window ~80% of viewport width, centered. Left ~10% shows
   the previous window (name order) peeking at reduced size.
   Right ~10% shows the next. Wraps around at the ends.
3. Top-bar `[◀]` `[▶]` cycle buttons (active only in Carousel mode).
4. Clicking a peek window jumps directly to it (becomes focused).
5. Minimized strip still available for agents the user has manually
   minimized out of the carousel.
6. Keyboard (via step 5 later): `←/→` call the cycle buttons when
   Carousel is active and no xterm has focus.

### Acceptance

- Toggling Carousel mode shows one centered primary + two peeks.
- `[◀]` and `[▶]` buttons cycle through agents by name order.
- Clicking a peek brings it to center.
- Pinned / minimized windows are excluded from the cycle.
- cargo test + clippy clean.
