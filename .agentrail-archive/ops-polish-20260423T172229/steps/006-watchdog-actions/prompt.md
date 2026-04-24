## Step 6: Watchdog actions — Ctrl-C, blocked_notice, UI badge

Turn the stall signal from step 5 into real consequences: nudge the
agent, post a blocked_notice for the coordinator, and paint the UI.

### Deliverables

1. Watchdog policy in atn-server:
   - First stall event: send Ctrl-C (canned action) to the agent's
     PTY. Log `watchdog.ctrl_c` event.
   - Repeat stall within 2 * stall_secs: post a `blocked_notice`
     PushEvent with `summary=\"agent stalled for Ns; watchdog sent
     Ctrl-C\"`. Route normally (should land on coordinator's inbox).
   - Optional third strike (configurable): restart.
2. `max_running_secs` path (separate from stall): if an agent is
   running uninterrupted for that long, escalate with a
   `blocked_notice`. No auto-restart by default.
3. UI: stalled windows paint an amber outline on their chrome +
   sparkline-row cell (`.stalled` class). Clears when state changes.
4. Add a `docs/windowed-ui.md` paragraph covering the watchdog
   affordance + how to tune `watchdog.stall_secs` per agent.
5. `docs/status.md` — remove the known-issue bullet about state
   tracking fragility once this ships.

### Acceptance

- Manually stall a fake-shim: watchdog sends Ctrl-C within
  stall_secs; second stall posts a blocked_notice to the
  coordinator inbox (confirm via `/api/events`).
- Dashboard shows amber outline on the stalled window.
- cargo test + clippy + doc clean.