# All Together Now — Implementation Plan

## Phase 0: Project Skeleton
**Goal**: Cargo workspace, crate boundaries, CI.

- [ ] Convert to Cargo workspace with crates: `atn-core`, `atn-pty`, `atn-server`, `atn-ui`, `atn-wiki`, `atn-trail`
- [ ] Define core types in `atn-core` (AgentConfig, AgentState, InputEvent, OutputEvent, PushEvent)
- [ ] Add dependencies: `portable-pty`, `tokio`, `axum`, `yew`, `serde`, `tracing`
- [ ] Placeholder `lib.rs` in each crate, `main.rs` in `atn-server`
- [ ] Basic CI: `cargo check --workspace`, `cargo clippy`, `cargo fmt --check`

## Phase 1: Single Agent PTY
**Goal**: Spawn one shell, capture output, inject input — validate the core PTY mechanism.

- [ ] Implement `PtySession` in `atn-pty`: spawn shell, clone reader, take writer
- [ ] Implement serialized writer queue (tokio mpsc → PTY master)
- [ ] Implement output reader task (PTY master → broadcast channel)
- [ ] Basic prompt detection (configurable pattern + idle timer)
- [ ] Send Ctrl-C (0x03) and text input
- [ ] Integration test: spawn `bash`, inject commands, verify output
- [ ] Transcript logging to file

## Phase 2: Minimal Web UI
**Goal**: One agent panel in the browser, streaming terminal output.

- [ ] Axum server with SSE endpoint for terminal bytes
- [ ] REST endpoints: send input, send Ctrl-C
- [ ] Yew app shell with single agent panel
- [ ] `xterm.js` integration for terminal rendering in browser
- [ ] Input box + Send button + Ctrl-C button
- [ ] Trunk build setup for WASM

## Phase 3: Multi-Agent Management
**Goal**: N agents, each with its own PTY and UI panel.

- [ ] `SessionManager` in `atn-pty`: spawn/track/shutdown N agents
- [ ] Agent config file (TOML) for defining agents
- [ ] Dashboard view: grid of agent panels
- [ ] Per-agent SSE streams
- [ ] Agent state machine with UI badges (Running, Awaiting Input, Idle, etc.)
- [ ] `PgmController` facade over session manager

## Phase 4: Wiki Integration
**Goal**: Shared coordination wiki accessible from UI and agents.

- [ ] Adapt wiki-rs `WikiStorage`/`AsyncWikiStorage` traits into `atn-wiki`
- [ ] File-based wiki backend (reuse wiki-rs implementation)
- [ ] Wiki REST endpoints (GET, PUT, PATCH, DELETE pages)
- [ ] CAS concurrency (ETag-based, from wiki-rs)
- [ ] Wiki browser component in Yew UI
- [ ] Markdown rendering with wiki-links
- [ ] Seed coordination pages: Goals, Agents, Requests, Blockers, Log

## Phase 5: Message Routing
**Goal**: Structured inter-agent communication through the PGM.

- [ ] Define push event types in `atn-core`
- [ ] `MessageRouter` trait + default implementation
- [ ] Agent push detection (parse structured output from PTY or side-channel file)
- [ ] Routing logic: known target → PTY inject; unknown → wiki + broadcast
- [ ] Human escalation for unroutable events
- [ ] Push event log (append-only, viewable in UI)
- [ ] "Awaiting input" detection heuristics for Claude Code output

## Phase 6: Agentrail Integration
**Goal**: Per-agent workflow tracking with ICRL.

- [ ] Adapt agentrail-rs core types into `atn-trail` (saga, step, trajectory, skill)
- [ ] Per-agent saga lifecycle managed by PGM
- [ ] Step completion → trajectory recording
- [ ] `agentrail next` equivalent: inject skill + experiences into fresh context
- [ ] UI: saga progress per agent (current step, step history)
- [ ] Skill distillation trigger (manual or periodic)

## Phase 7: Polish & Robustness
**Goal**: Production-quality session management and UX.

- [ ] Shutdown sequence: double Ctrl-C, SIGTERM fallback
- [ ] Session restart / reattach after crash
- [ ] Dependency graph visualization (which agent blocks which)
- [ ] Notification sounds / desktop notifications for attention events
- [ ] Agent config hot-reload
- [ ] Comprehensive logging and error handling
- [ ] Documentation and usage guide updates

## Future: Alternative Frontends
Not in MVP scope, but the library-first design enables:

- **CLI frontend**: `atn-cli` crate using `PgmController` directly
- **TUI frontend**: `atn-tui` crate using `ratatui` + `crossterm`
- **Emacs frontend**: `atn-emacs` crate using `emacs-module-rs`

## Dependencies Between Phases

```
Phase 0 ──→ Phase 1 ──→ Phase 2 ──→ Phase 3 ──┐
                                                 ├──→ Phase 7
                          Phase 4 ──→ Phase 5 ──┘
                          Phase 6 ──────────────┘
```

Phases 4 and 6 can proceed in parallel with Phase 3. Phase 5 depends on
both Phase 3 (multi-agent) and Phase 4 (wiki). Phase 7 integrates everything.

## Estimated Complexity

| Phase | Crates Touched | Key Risk |
|-------|---------------|----------|
| 0 | all | None — scaffolding |
| 1 | atn-pty, atn-core | PTY portability edge cases |
| 2 | atn-server, atn-ui | xterm.js ↔ Yew interop |
| 3 | atn-pty, atn-server, atn-ui, atn-core | State synchronization |
| 4 | atn-wiki, atn-server, atn-ui | CAS conflict handling |
| 5 | atn-core, atn-pty, atn-server | Prompt detection reliability |
| 6 | atn-trail, atn-server, atn-ui | Agentrail API adaptation |
| 7 | all | Integration edge cases |
