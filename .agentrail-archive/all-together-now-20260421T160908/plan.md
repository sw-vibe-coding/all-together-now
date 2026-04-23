All Together Now — Scale-UI Saga

Goal: make the ATN dashboard usable with many agents (20+). Big focused
terminal + treemap of the rest, sized by "heat" (recent activity + state
signals). Swap tiles to focus with a click or keyboard. Xterm stays at fixed
native dims; CSS scaling does the visual layout so PTY size only changes on
explicit focus.

Non-goals (other sagas):
  - git-sync / PR-equivalent flow
  - config editor SpawnSpec refactor
  - auth / multi-user
  - CUDA / Ollama integration

Steps:

1. heat-score
   Per-agent heat tracking on the server. EWMA of output bytes/sec + state
   multipliers (awaiting_human_input, error, blocked amplified; disconnected
   muted). Decays to a floor. `GET /api/agents/heat` returns an array of
   { id, heat, bytes_per_sec, state_boost }. Heat tracker wired into every
   spawn path: startup, create, restart, reconnect, hot-reload.

2. compact-tile
   New compact panel variant for sub-threshold tiles: role + name, state
   badge, last-line preview, bytes-per-sec sparkline, click-to-focus. No
   xterm inside. Replaces current rendering whenever a tile is too small
   for xterm to be readable.

3. css-scaled-xterm
   Render xterm at fixed native rows/cols (e.g. 120x40); wrap in a
   `term-scale` div that gets `transform: scale(k)` based on tile size.
   Snap k to a discrete set to avoid sub-pixel blur. PTY resize no longer
   fires on every layout shuffle — only on pin-to-focus. Keeps TUI agents
   honest (they see a stable size). Unit-test the scale picker.

4. treemap-layout
   Squarified treemap replacing the current cols-1..cols-4 grid. Focus
   panel takes ~1/2 of viewport; remaining real estate is a treemap of the
   other agents, tile area proportional to smoothed heat. Minimum tile
   size falls back to compact-tile view. Layout refresh cadence 1-2s, not
   per-byte. Freeze-on-interaction timer.

5. swap-pin-kbd
   Click any tile to swap it into the focus panel; the displaced focus
   tile drops back into the treemap. Pin row above the treemap for agents
   the user wants to keep at a stable size. Keyboard: digits to focus top-N
   hot agents, `f` to toggle focus, `p` to pin.

6. search-filter-groups
   `/` opens a fuzzy filter over the tile set (matches on name, project,
   role). Chips for role/transport/project filters. Optional grouping
   so the treemap packs by group. Saved layouts (named filter+pin sets).

7. scale-demo-and-docs
   `demos/scale/setup.sh` spins up N fake agents with varied activity
   profiles (spammer, quiet, periodic bursts, awaiting-input, error) so
   the layout gets exercised. Updates docs/usage.md and a new docs/scale-ui.md.
