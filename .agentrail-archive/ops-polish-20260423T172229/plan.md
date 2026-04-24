ATN — Ops Polish

Operational robustness + visibility improvements that stack cleanly:
(a) shell-escape fix for coordinator commands, (b) terminal snapshot
tooling so diagnostic captures don't require a replay crate invocation,
and (c) per-agent watchdog for stall/liveness detection.

## Steps

1. priority-escape — fix the `(priority: High)` shell syntax error.
   Canned-command formatting bakes raw values into shell-interpreted
   strings; parens, `<`, `>`, `$`, `"`, `'`, `\`, spaces can all break
   the PTY write. Audit every `coordinator_command` / canned-action
   emitter (CannedAction, any `format!` that builds a shell line),
   introduce a `shell_escape` helper (prefer single-quote-then-insert
   style), add table-driven unit tests for the escapers. Fixes the
   known-issue in docs/status.md.

2. pty-screenshot-core — `vte`-driven terminal snapshot. New module
   (likely under `atn-pty` as `snapshot.rs`, or a thin shared helper
   used by both atn-server and atn-replay) that takes the last-N
   transcript bytes (or a live stream tap) and renders the current
   terminal grid as plain text / ansi / html. Factor out anything
   that's already in atn-replay to avoid duplication.

3. pty-screenshot-endpoint — `GET /api/agents/{id}/screenshot?format=text|ansi|html&rows=40&cols=120`,
   backed by the current transcript. Returns text/plain by default.
   Stable shape so atn-cli (next saga) can wrap it. Integration test
   against the 3-agent demo fixture.

4. pty-screenshot-ui — `📸` icon in each window chrome next to the
   existing action icons; click opens the snapshot in a new tab (so
   the user can copy/paste the text). Hovering the icon shows a
   tooltip with the endpoint URL. `window.snapshotAgent(id)` exposed
   for devtools.

5. watchdog-core — per-agent output-stall detection (>N seconds no
   output while state is `running`) + process-liveness check.
   Configurable per agent (`watchdog.stall_secs`,
   `watchdog.max_running_secs` in SpawnSpec). Pipes a new
   `OutputSignal::Stalled` into the state tracker so it's visible
   alongside Disconnected/Idle.

6. watchdog-actions — on stall, send Ctrl-C; on repeat-stall within
   a window, post a `blocked_notice` push event (routes to
   coordinator) and optionally restart the agent. Stalled state
   paints an amber outline on the window chrome + the sparkline-row
   cell. Dashboard shows a "stalled" badge near the state chip.

## Success metrics

- Canned commands with `(priority: X)` round-trip through a bash PTY
  intact (verified via integration test).
- `/api/agents/<id>/screenshot` returns a useful plain-text snapshot
  for any agent with ≥1 line of output.
- A manually-stalled fake-shim agent triggers Ctrl-C within
  watchdog.stall_secs and posts a `blocked_notice`.
- cargo test + clippy + doc clean.
