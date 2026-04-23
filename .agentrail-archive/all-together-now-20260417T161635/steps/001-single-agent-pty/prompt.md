## Phase 1: Single Agent PTY — Integration Tests and Validation

Phase 0 (workspace skeleton, core types, wiki storage, trail reader) is already implemented and passing all 18 tests.

### Deliverables

1. **PTY integration tests** in `crates/atn-pty/tests/`:
   - `spawn_and_echo`: spawn bash via PtySession, send `echo hello`, verify output contains "hello"
   - `ctrl_c_interrupt`: spawn bash, send `sleep 300`, send Ctrl-C, verify prompt returns
   - `shutdown_clean`: spawn bash with long command, call shutdown(), verify clean exit

2. **Transcript logging** in `crates/atn-pty/src/`:
   - Add a `transcript.rs` module that appends raw PTY output to `{log_dir}/{agent_id}/transcript.log`
   - Structured events logged to `{log_dir}/events.jsonl`

3. **SessionManager** in `crates/atn-pty/src/manager.rs`:
   - Manages lifecycle of multiple PtySession instances
   - spawn_agent(config) -> AgentId
   - get_session(id) -> &PtySession
   - shutdown_agent(id)
   - shutdown_all()

4. **PgmController facade** in `crates/atn-core/src/controller.rs`:
   - Public API that wraps SessionManager + wiki + trail
   - This is the library boundary for future CLI/TUI/Emacs frontends

### Acceptance Criteria

- `cargo test --workspace` passes with new PTY integration tests
- `cargo clippy --workspace -- -D warnings` clean
- PTY tests spawn real bash processes and verify I/O
- Transcript files are created in a temp directory during tests

### Constraints

- Use `tokio::task::spawn_blocking` for PTY reader/writer (blocking I/O)
- Use `tokio::sync::broadcast` for output, `tokio::sync::mpsc` for input
- Serialized writer per agent — no interleaved writes
