# Windowed UI

The dashboard's primary model is a **desktop window manager** with three
layout modes, click-to-select focus, and bare-key keyboard shortcuts. It
replaces the heat-driven treemap (still reachable; see
[scale-ui.md](./scale-ui.md)) for the "handful of long-lived agents"
use case that's become ATN's common cadence.

## TL;DR

- **Layouts**: Tiled / Stack / Carousel — picked in the top bar.
- **Chrome per window**: minimize, maximize/restore, pin, config,
  reconnect, delete.
- **Click header → selected** (accent outline). Bare keys operate on
  the selected window. **Click inside xterm → keys go to the PTY**; an
  amber **typing to PTY** badge appears near Send.
- **Pin = lock-in-place**. A pinned window keeps its rect through
  every layout op; unpin with the same chrome button.
- **Sparkline row** at the top shows one flex cell (~1/n width) per
  agent — live sparkline + last line. Click a cell to focus that agent.

## Layout modes

### Tiled (default)

Coordinator on the left at ~55% of viewport width; workers tile on the
right in `sortMode` order (name by default). Equal-sized worker tiles.
No heat-based sizing, no auto-refreshing layout churn.

### Stack

One **primary** window centered at ~80% × 90%; every other agent drops
to the bottom dock strip. Switching to Stack auto-minimizes non-primary
agents; clicking a dock cell swaps the clicked agent into the primary
slot. Toggling back to Tiled restores every non-pinned window.

### Carousel

One focused window at ~80% center + a ~9% peek of the previous and
next agent on either side. `◀` / `▶` buttons in the top bar cycle
through the name-ordered ring (wraps at the ends). Peek click jumps
that agent to center. Manually-minimized agents skip the cycle and
stay in the bottom dock.

## Per-window chrome

| Icon | Action                                     |
|------|--------------------------------------------|
| `_`  | **Minimize** to the bottom dock            |
| `□`  | **Maximize** to ~80% × 90% (toggle)        |
| `📌` | **Pin / unpin** — lock the current rect    |
| `▸`  | **Config** — inline spec editor            |
| `↻`  | **Reconnect** — hard re-attach (mosh+tmux) |
| `✕`  | **Delete** — tear the agent down           |

Clicking anywhere on the header (not on an icon) **selects** the
window. Selected = accent green outline + target of bare-key shortcuts.
Clicking inside the xterm body routes all keys to the PTY instead.

### Pin semantics

Pin is **lock-in-place**. The first click snapshots the window's
current `{x, y, w, h}` into `layoutState.windows[id].pinnedRect`.
Subsequent layout operations — switching Tiled↔Stack↔Carousel, dock
promotion, focus cycling — skip the pinned window entirely. It floats
at the stored rect above the normal tiles (`z-index: 10`) with an
amber outline.

Unpin: click the 📌 again. The `pinnedRect` is discarded and the
window rejoins the layout pool.

## Top-bar controls

| Control           | Effect                                 |
|-------------------|----------------------------------------|
| Layout selector   | Tiled / Stack / Carousel               |
| Sort selector     | Name / Recent (smoothed heat desc)     |
| `◀` / `▶`         | Carousel cycle (visible in Carousel)   |
| `kbd-hint` strip  | Inline reminder of the bare keys       |

No **Refresh** button. The dashboard never auto-recomputes the layout
— every visible change comes from an explicit user action or an agent
being created/deleted.

## Keyboard (Option C)

Bare-key bindings. They fire only when no `<input>`, `<textarea>`,
`<select>`, or xterm element has focus.

| Key     | Action                                                   |
|---------|----------------------------------------------------------|
| `m`     | Minimize the selected window                             |
| `M`     | Maximize / restore toggle on the selected window         |
| `p`     | Pin / unpin the selected window                          |
| `←/→`   | Cycle focus through non-minimized windows (sort order)   |
| `1..9`  | Jump to Nth window by sort order                         |
| `/`     | Focus the filter input                                   |
| `Esc`   | Restore any maximized window; second Esc deselects       |

When focus lands inside a window's xterm, that panel's Send row shows
an amber **typing to PTY** badge. Keys are routed to the PTY until
focus leaves the xterm.

## Sparkline row

Above the dashboard sits a sparkline strip with one cell per agent.
Each cell is `flex: 1 1 0` so three agents split the row in thirds,
nine agents take a ninth each, etc. Per cell:

- Name + role + state badge
- 60 s inline SVG sparkline of bytes/sec
- Last line of output (ANSI-stripped, ≤ 200 chars)
- Amber 📌 + outline if the agent is pinned

Click a cell → `swap-to-focus` → the agent becomes the selected window
(and in Carousel / Stack, the focused primary).

## Persistence

Windowed-UI state lives under `localStorage` key **`atn-window-ui-v1`**:

```jsonc
{
  "layoutMode": "stack",
  "sortMode": "name",
  "selectedId": "worker-hlasm",
  "windows": {
    "coordinator":   { "state": "normal", "pinned": true,
                       "pinnedRect": { "x": 4, "y": 4, "w": 763, "h": 735 } },
    "worker-hlasm":  { "state": "minimized" }
  }
}
```

Only non-default windows are serialized (a window with
`state: "normal"` and `pinned: false` is omitted). A hard-refresh
restores:

- Layout mode (tiled / stack / carousel)
- Sort mode (name / recent)
- Selected window
- Per-window minimize/maximize state
- Pinned set + each pinned window's locked rect

The legacy scale-UI bits (filter text, chips, group-by, saved named
layouts, focus-size) continue to live under
`atn-scale-ui-v1` — the two keys are independent.

## Demo

See [Demo 9 in docs/demos-scripts.md](./demos-scripts.md#demo-9--windowed-ui)
for a scripted 4-agent walkthrough that exercises every layout mode,
the pin lifecycle, and the keyboard bindings in ~5 minutes.

Boot it directly:

```bash
./demos/windowed-ui/setup.sh
```

## Relationship to scale-UI

The scale-UI (heat-sized treemap, pin row strip, auto-focus) is still
served from the same binary and all the underlying plumbing —
compact tiles, CSS-scaled xterm, the `/api/agents/heat` endpoint —
is shared. The windowed UI is the new **primary** model for 3–10
agent workloads; the treemap is the scale-UI model for 20+ agent
fleets where tile-area-by-heat becomes load-bearing again.

See also:
- [docs/scale-ui.md](./scale-ui.md) — 21-agent scale-UI fleet
  walkthrough (legacy model, still functional)
- [docs/usage.md](./usage.md) — operational guide
- [docs/demos-scripts.md](./demos-scripts.md) — every demo the build
  supports today
