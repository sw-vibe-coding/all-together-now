## Step 5: Keyboard — Option C

Bare-key shortcuts that only fire when no xterm / input / textarea
currently has focus. Matches the desktop-manager model: click
header = selected, keys apply; click xterm body = keys go to PTY.

### Deliverables

1. Global keydown listener, guarded by `isTypingTarget(event.target)`
   (already exists). Additional check: skip when `document.activeElement`
   is inside any `.xterm` element (some browsers fire keys on
   document when xterm has focus).
2. Key bindings (unmodified):
   - `m` → minimize the selected window
   - `M` (shift-m) → maximize / restore toggle on selected
   - `p` → pin / unpin selected
   - `←/→` → cycle focus to prev / next window (by sort order,
     excluding minimized and pinned-but-minimized)
   - `1..9` → jump focus to the Nth window by sort order
   - `Esc` → deselect (removes accent), focus nothing; if primary
     is maximized, restore.
3. Top bar shows a small hint: "click header to select, type to
   command — bindings: m, M, p, ←→, 1..9, Esc". Toggle in settings
   later; for now always visible as muted text.
4. When focus is clearly INSIDE an xterm (xterm's element.focused),
   a tiny "typing into terminal" badge appears near the Send button
   as an always-on indicator.

### Acceptance

- With coord selected and no xterm focus: `m` minimizes it.
- Clicking into a worker's xterm and typing `m` → the letter `m`
  shows up in the terminal; no window action.
- `Esc` deselects and restores any maximized window.
- `1` focuses the first window in sort order; `9` does nothing if
  fewer than 9 windows.
- cargo test + clippy clean.
