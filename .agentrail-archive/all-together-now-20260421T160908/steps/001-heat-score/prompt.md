## Step 1: Heat Score

Server-side "heat" tracking for every agent. Downstream steps use it to
drive tile size in the map-of-market layout, but step 1 is purely the data
plumbing and a readable endpoint.

### Deliverables

1. New module (suggested: `crates/atn-server/src/heat.rs`) with:
   - `HeatState` — per-agent EWMA of bytes-per-second plus bookkeeping
     (last update timestamp, totals). EWMA half-life ~30 s is a reasonable
     default; expose it as a constant.
   - `HeatMap = Arc<Mutex<HashMap<String, HeatState>>>` shared state.
   - `spawn_heat_tracker(rx, heat_map, id)` task that receives
     `OutputSignal::Bytes` and updates the EWMA. One per agent.
   - `compute_score(state, &AgentState, pins)` pure function that mixes the
     byte-rate EWMA with state boosts:
       - awaiting_human_input → high boost
       - error, blocked → high boost (pulsing UI later)
       - disconnected → muted (low weight)
       - starting/idle/running → neutral
     Returns a normalized score (e.g. f32 in [0.0, 1.0]).
   - Decay: if last update was >N seconds ago, the EWMA naturally decays
     because elapsed-time weighting in `update(bytes, now)` does the work.
2. Wire into every spawn path so heat tracking starts at the same moment
   the session does:
   - startup loader
   - `create_agent`
   - `agent_restart`
   - `agent_reconnect`
   - hot-reload
   Stop tracking (remove map entry) on `delete_agent` and hot-reload
   removal.
3. `GET /api/agents/heat` returns a JSON array, one entry per live agent:
   `{ "id", "heat", "bytes_per_sec", "state_boost" }`. Keep the shape flat
   so step 4's treemap code can `fetch().then(list => ...)` trivially.
4. Unit tests for `HeatState` EWMA math (bursts push it up, quiet periods
   decay it) and `compute_score` (state boosts layer on correctly).

### Acceptance

- `cargo test --workspace` green; `cargo clippy --workspace --all-targets
  -- -D warnings` clean.
- Live smoke: spawn a fake-claude agent, assert heat rises while it emits
  its startup banner, decays toward the floor after stdin blocks.
- `GET /api/agents/heat` returns entries for every live agent, updates as
  new bytes arrive.
