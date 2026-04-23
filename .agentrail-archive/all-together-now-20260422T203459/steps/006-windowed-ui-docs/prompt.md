## Step 6: Persistence, Docs, Demo

Final polish: wire localStorage for the new state shape, update
docs, add a demo, mark old scale-UI docs as legacy.

### Deliverables

1. localStorage key `atn-window-ui-v1` persists:
   - `layoutMode` (tiled / stack / carousel)
   - `sortMode` (name / recent)
   - `selectedId`
   - Per-agent `windows[id] = { state, pinned }` for states that
     diverge from defaults
2. `docs/windowed-ui.md` — walkthrough covering: layouts, chrome
   controls, click-to-select, keyboard Option C, sort modes,
   pin/minimize/maximize lifecycle, saved layouts persistence.
3. `docs/demos-scripts.md` gains Demo 9 "Windowed UI" and Demo 6
   (treemap scale-UI) is marked `(legacy)` with a pointer at the
   new doc.
4. `docs/scale-ui.md` gains a banner at the top linking to
   `windowed-ui.md` and noting it's the preferred model now.
5. `docs/usage.md` Dashboard section rewritten around windows.
6. `docs/status.md` adds W1..W6 rows for this saga.
7. `demos/windowed-ui/setup.sh` — identical topology to
   `demos/claude-opencode/setup.sh` (1 coord + 3 workers) but
   written to showcase the windowed model (post-message hinting
   the user to try minimizing, switching layouts, keyboard
   shortcuts).

### Acceptance

- Setting layoutMode/sortMode/pins survives a browser hard-refresh.
- Docs are followable end-to-end.
- cargo doc --workspace --no-deps warning-free.
