## Phase 8: Session Management UI

Full agent lifecycle control from the browser — create, configure, start, stop, and destroy agents without editing TOML.

### Deliverables

1. Agent CRUD REST endpoints (`POST /api/agents`, `DELETE /api/agents/{id}`, `PUT /api/agents/{id}`)
2. Dynamic SessionManager mutations (spawn/shutdown while server is running)
3. "New Agent" form in both static HTML and Yew UI (name, repo path, role, setup commands, launch command)
4. Per-agent directory picker / path input
5. Start/stop/restart controls per agent in the dashboard
6. Persist dynamically-created agents back to agents.toml (optional save)
7. Agent configuration editing from the UI (change working dir, role, commands)

### Acceptance Criteria

- Can create a new agent session from the browser without restarting the server
- Can stop and restart individual agents from the UI
- Can change an agent's working directory from the UI
- Agent list updates in real-time after create/destroy
- cargo test --workspace passes
- cargo clippy --workspace -- -D warnings clean
