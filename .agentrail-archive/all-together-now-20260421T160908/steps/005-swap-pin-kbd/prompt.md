## Step 5: Swap, Pin, Keyboard

User-facing controls for rearranging the treemap.

### Deliverables

1. Click-to-swap (step 4 fires the event; step 5 wires it end-to-end):
   clicking a non-focus tile promotes it into the focus panel; the old
   focus drops back into the treemap retaining its heat-based size.
2. Pin row above the treemap: horizontal strip of fixed-width cells for
   agents the user explicitly pinned. Pins ignore heat; their size is
   stable (default ~200×160 px). Up to N pins (tune: 6). Unpin by
   clicking the × on the pinned cell.
3. Pin/unpin controls on every tile (hover menu: Pin, Focus, Delete) and
   on the focus panel.
4. Keyboard shortcuts:
   - `1..9` → focus the Nth hottest tile
   - `0` → focus the coordinator (if one exists)
   - `f` → toggle focus panel size (medium / large)
   - `p` → pin/unpin the currently-focused tile
   - `/` → open the search/filter input (step 6 binds the search logic;
     step 5 just focuses the input)
   - `Esc` → clear focus, restore auto-pick
5. Persist pins + focus choice in localStorage so refreshes keep the
   current layout.
6. Visual affordances: pinned tiles get a small pin icon; focus panel
   gets a subtle border.

### Acceptance

- Pressing `1` focuses the hottest non-coordinator tile; swap animations
  happen without xterm re-init (scrollback survives).
- Pinning survives a browser refresh.
- `Esc` restores auto-pick behavior.
- cargo test / clippy clean.
