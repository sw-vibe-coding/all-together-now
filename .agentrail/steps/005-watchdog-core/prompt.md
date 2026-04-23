## Step 5: Per-agent output-stall watchdog

Detect agents that have gone quiet while nominally `running` and
surface it as a first-class signal. Foundation for step 6's auto-
Ctrl-C + blocked_notice path.

### Deliverables

1. New `Watchdog` struct in `atn-pty` (or `atn-core` if it fits pure
   domain types) tracking:
   - `last_output_at: Instant` per agent
   - `stall_secs: u64` (default 60)
   - `max_running_secs: u64` (default 600, optional)
2. SpawnSpec gains optional `[watchdog]` fields:
   `stall_secs`, `max_running_secs`. Defaults apply when absent.
3. New `OutputSignal::Stalled` variant (or equivalent on the state
   tracker) fires when `Instant::now() - last_output_at > stall_secs`
   AND state is `running`. Clears when output resumes.
4. State tracker owns the watchdog timer; spawn-time wiring in
   atn-server rehydrates the watchdog from the spec.
5. Unit tests in atn-pty: simulate bursts of output + quiet gaps,
   assert Stalled fires exactly once per stall event.

### Acceptance

- `GET /api/agents/<id>/state` reports `{state: 'running', stalled:
  true, stalled_for_secs: N}` when a fake-shim is blocked.
- cargo test + clippy clean.
- No false positives for agents legitimately `idle` or `awaiting_human_input`.