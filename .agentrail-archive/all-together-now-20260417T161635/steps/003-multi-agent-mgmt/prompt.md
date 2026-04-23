## Phase 3: Multi-Agent Management

Scale from one agent to N agents with a dashboard grid.

### Deliverables

1. Agent config file (agents.toml) for defining multiple agents
2. Dashboard view: responsive grid of agent panels in Yew
3. Per-agent SSE streams
4. Agent state machine with color-coded UI badges (Running, Awaiting Input, Idle, etc.)
5. Session manager handling N concurrent PTY sessions

### Acceptance Criteria

- Configure 2-3 agents via agents.toml, all appear in dashboard
- Each agent has independent terminal stream and input
- State badges update based on output parsing heuristics
- cargo test --workspace passes
