## Step 2: Compact Tile

Small-footprint tile variant used whenever an agent's allotted space is too
small for an xterm to be readable. Displays role + name, state badge, a
1-line sparkline driven by the step-1 bytes-per-sec stream, last output
line, and clicks to focus.

### Deliverables

1. New DOM template in the static dashboard: `<div class="agent-compact">`
   with header row (name + role + state badge) and content row (sparkline
   + truncated last-line + click-to-focus hint).
2. JS helper `compactTile(agent)` that renders the compact layout.
3. SSE handler captures the most recent line of output per agent (rolling
   buffer, last 200 chars is plenty) and exposes it to the compact renderer.
4. Poll `/api/agents/heat` at 1 Hz; drive a small canvas/svg sparkline per
   compact tile from the bytes-per-sec history (60-sample rolling window).
5. Click anywhere on a compact tile fires a `swap-to-focus` custom event.
   Step 4 wires the layout side; for now just console.log.
6. Make it easy to toggle a single panel between compact and xterm mode
   via a JS switch — step 4 picks the mode based on allotted size.

### Acceptance

- With 3+ agents, manually toggling compact mode renders cleanly and the
  sparkline updates in real time.
- No xterm DOM lives inside compact tiles (cheap to render many of them).
- click → console.log fires with the right agent id.
- Workspace test suite unaffected; clippy clean.
