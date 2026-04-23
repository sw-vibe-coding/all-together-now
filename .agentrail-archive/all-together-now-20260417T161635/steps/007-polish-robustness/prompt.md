## Phase 7: Polish and Robustness

Production-quality session management and UX.

### Deliverables

1. Shutdown sequence: double Ctrl-C, SIGTERM fallback, clean child exit
2. Session restart / reattach after crash
3. Dependency graph visualization (which agent blocks which)
4. Notification sounds / desktop notifications for attention events
5. Agent config hot-reload
6. Comprehensive error handling and logging
7. Documentation updates (usage.md, status.md)

### Acceptance Criteria

- Graceful shutdown of all agents on server exit
- UI shows dependency graph
- All docs updated to reflect final implementation
- cargo test --workspace passes
- cargo clippy --workspace -- -D warnings clean
