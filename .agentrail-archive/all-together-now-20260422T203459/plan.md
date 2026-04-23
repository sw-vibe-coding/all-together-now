ATN — Windowed-UI Redesign

The scale-UI treemap worked in theory but drifts in practice: heat
decay shifts tile sizes, squarify flips ordering on ties, scale-snap
picks oscillate, and users get stuck in states they can't recover from.
Replace it with a conventional desktop-windowing model.

## Design (agreed with user)

Window states (per agent): `normal` | `minimized` | `maximized`.

Layout modes (mutually exclusive):
  - **Tiled** — grid of equal tiles; coord top-left, workers by
    sort order
  - **Stack** — one primary window ~80%; everything else minimized
    to a strip
  - **Carousel** — one focused window ~80% + peek of prev/next;
    ◀/▶ cycles

Sort: `name` (default, coord-first) | `recent` (most-recent-output
first; heat endpoint keeps powering this, no tile-area impact).

Per-window chrome: always-visible header with [_] minimize,
[□]/[❐] maximize/restore toggle, [📌] pin toggle, [✕] delete,
[▸] config. Plus the existing Send/Ctrl-C/Restart/Reconnect/Stop
controls below the xterm.

Top-bar controls: layout selector, sort selector, [◀] [▶] focus
cycler. No Refresh button — nothing auto-updates; any UI change is
an explicit user action or agent add/remove.

Keyboard (option C — click-to-select, no mode):
  - Clicking a window's **header bar** selects it (accent outline).
  - Bare keys `m` / `M` / `p` / `←/→` / `1..9` / `Esc` only act
    when no input/textarea/xterm currently has focus.
  - Clicking **inside** an xterm puts focus there; all keys route
    to the PTY. Clicking empty dashboard or pressing `Esc` frees.
  - The **Send** field below each xterm remains the primary text
    path to agents, so most users never click into xterm directly.

Defaults: layout = **Stack**, sort = **name**. Coord is "the most
prominent window" in every layout — it's top-left in Tiled,
primary in Stack, center in Carousel's initial position.

## Steps

1. **tiled-foundation** — strip the treemap/squarify/heat-sizing
   machinery; replace with a simple tiled-grid placement function
   (coord top-left, workers in name order, equal-sized). Keep the
   existing per-panel controls intact for now. localStorage carries
   `layoutMode: 'tiled'` (only value supported this step) and
   `sortMode: 'name'`.

2. **window-chrome** — new titlebar per window with minimize /
   maximize-restore / pin / delete / config icons. Click header to
   select the window (accent outline); click xterm body to focus the
   PTY. Minimize collapses to a bottom strip cell (live state badge
   + last-line preview). Maximize fills ~80% of viewport, pushing
   others to minimized. Restore brings them back.

3. **stack-layout** — add the Stack mode: one primary window
   at ~80% viewport + strip of minimized windows at bottom. Primary
   = focused/selected, else coord. Switching from tiled → stack
   auto-minimizes non-primary.

4. **carousel-layout** — add Carousel: focused window ~80% center,
   prev/next peek at edges. ◀/▶ cycle buttons in top bar.

5. **keyboard-option-C** — bare-key shortcuts act on the currently
   selected window (or the focused in carousel). `m` minimize, `M`
   maximize toggle, `p` pin toggle, `←/→` focus prev/next, `1..9`
   jump, `Esc` deselect / restore-all. Guard: isTypingTarget skips
   when xterm/input/textarea has focus. Keyboard hint line in the
   top bar.

6. **persistence-docs-demo** — localStorage persists layoutMode,
   sortMode, pinned set, per-window state. docs/demos-scripts.md
   gets a new Demo 9 "windowed UI" and demo 6 (treemap) gets an
   "archived" note. docs/windowed-ui.md walkthrough. Old
   scale-ui.md stays but is linked as legacy.
