## Step 2: Window Chrome + Click-to-Select

Add always-visible window controls. Click the header to select;
click the xterm body to route keys to the PTY. Minimized windows
collapse to a bottom strip with live state updates.

### Deliverables

1. Window header gets six icon buttons (small, on the right):
   - `_` minimize
   - `□` / `❐` maximize / restore toggle
   - `📌` pin / unpin toggle
   - `▸` config (opens the existing SpawnSpec config editor)
   - `↻` reconnect
   - `✕` delete (with a confirm step)
   Restart + Stop stay in the controls row below the xterm.
2. Per-agent `layoutState.windows[id] = { state: 'normal'|'minimized'|'maximized', pinned: boolean }`.
3. Click-to-select: clicking the header sets `layoutState.selectedId = id`.
   Panel gets a `.selected` class → accent outline.
4. Minimize collapses the panel to a fixed-size cell (~180×48) in a
   dashboard footer strip, showing role + name + state badge +
   last-line preview. Maximize expands the panel to ~80% of viewport
   (centered); others become minimized until restored.
5. Selected tile keeps `--term-k: 1.0` and fits xterm to its container
   via fitAddon + syncPtySize (like step-3 focus used to).
6. Minimized strip container (`#window-dock`) always visible at
   bottom when any window is minimized; hidden when none.
7. Maximizing an already-maximized window restores it (toggle).
   Same for minimize → restore.

### Acceptance

- Clicking the header outlines the panel in accent color; other
  panels lose the outline.
- Clicking a minimize icon collapses that panel; its cell appears
  in the footer strip. Clicking the cell restores.
- Maximize brings the panel to ~80% viewport; other visible panels
  minimize. Clicking the restore icon brings everything back.
- Pin icon toggles `.pinned` class + persists.
- Delete prompts and calls DELETE /api/agents.
- cargo test + clippy clean.
