# Changes

- Phase 8: Session management UI — agent CRUD REST endpoints (POST/PUT/DELETE /api/agents), stop endpoint, save-to-TOML endpoint, "New Agent" form in browser, per-agent stop/delete/config-edit controls, dynamic dashboard refresh on create/destroy
- Phase 7: Polish and robustness — graceful SIGINT/SIGTERM shutdown, agent restart endpoint + UI button, dependency graph visualization (Graph tab with SVG), notification sounds via Web Audio API, agents.toml hot-reload with file watcher, panic hook + structured logging, documentation updates
- Phase 6: Agentrail integration with Saga tab UI, per-agent saga badges, step progress timeline, trajectory viewer, skill distill button, REST API (GET /api/saga, GET /api/agents/{id}/saga, POST /api/saga/distill)
- Phase 5: Message routing with outbox polling, PushEvent routing (deliver/escalate/broadcast), event log REST API, Events tab in UI
- Phase 4: Wiki integration with REST API (GET/PUT/PATCH/DELETE), ETag CAS, browser UI in both static HTML and Yew
- Phase 3: Multi-agent management with dashboard grid and state tracking
- Phase 2: Minimal web UI with SSE terminal streaming
- Phase 0+1: Workspace skeleton with PTY integration tests
