# All Together Now — Status

## Current State: Remote-Agent Demo Saga Complete

Phases 0–8 shipped plus the follow-on **remote-agent demo saga** (5 steps,
all shipped in this sequence):

1. **empty-start** — ATN boots with zero agents; UI renders empty state
   with a + New Agent CTA. `agents.toml` defaults to `[project]`-only; seed
   preserved as `agents.example.toml`.
2. **new-agent-dialog** — structured `SpawnSpec` (name/role/transport/
   host/user/working_dir/project/agent/agent_args) in `atn-core`; `POST
   /api/agents` validates and composes shell commands for local/mosh/ssh.
   Both the Yew modal and the static HTML form use the same schema.
3. **remote-pty-transport** — `OutputSignal::Disconnected` signal pipes
   PTY EOF → `AgentState::Disconnected`; `POST /api/agents/{id}/reconnect`
   re-attaches to remote tmux; graceful delete sends `^B :kill-session` to
   clean up tmux server-side. `tools/fake-mosh` + symlinks back the
   integration tests.
4. **three-agent-demo** — `tools/fake-claude`, `tools/fake-codex`,
   `tools/fake-opencode-glm5` shims; `demos/three-agent/fixtures/*.json`
   + `setup.sh` (`ATN_DEMO_REAL=1` for real CLIs); `ATN_PORT` env +
   `atn-server ready on <addr>` stdout marker; `crates/atn-server/tests/
   three_agent_demo.rs` end-to-end integration test.
5. **docs-refresh** — this doc + `docs/usage.md`, `docs/demo-three-agent.md`,
   `docs/uber-use-case.md` cross-links, README quickstart.

End-to-end multi-agent demo working with reg-rs regression test.

## What Exists

| Crate | Status | Description |
|-------|--------|-------------|
| `atn-core` | Complete | Domain types: agent, event, inbox, error, config, router |
| `atn-pty` | Complete | PTY session management, reader, writer (with input logging), state tracker, transcript |
| `atn-server` | Complete | Axum HTTP/SSE server with all REST endpoints + agent CRUD |
| `atn-wiki` | Complete | FileWikiStorage + coordination page seeding |
| `atn-trail` | Complete | Agentrail file reader + CLI wrapper |
| `atn-replay` | Complete | PTY transcript viewer: screenshot, dashboard (org-mode), HTML output |
| `atn-ui` | Placeholder | Yew frontend (server uses embedded static HTML) |

## Phase Summary

| Phase | Description | Status |
|-------|-------------|--------|
| 0 | Project skeleton, workspace, core types | Done |
| 1 | Single agent PTY with integration tests | Done |
| 2 | Minimal web UI with xterm.js + SSE | Done |
| 3 | Multi-agent management with dashboard grid | Done |
| 4 | Wiki integration with REST API + CAS | Done |
| 5 | Message routing with outbox polling | Done |
| 6 | Agentrail integration with saga UI | Done |
| 7 | Polish: shutdown, restart, graph, notifications, hot-reload | Done |
| 8 | Session management UI (agent CRUD from browser) | Done |
| — | Demo, input logging, atn-replay crate | Done |
| R1 | Empty-start: boot with zero agents | Done |
| R2 | New Agent dialog: structured SpawnSpec + compose | Done |
| R3 | Remote PTY transport: reconnect + tmux cleanup | Done |
| R4 | Three-agent demo: fake shims + integration test | Done |
| R5 | Docs refresh: usage, demo walkthrough, cross-links | Done |
| S1..S5 | Scale-UI saga: heat score, compact tile, CSS-scaled xterm, treemap, pin + keyboard | Done |
| S6 | Demo scripts: docs/demos-scripts.md menu of runnable demos | Done |

## Demo

Two AI agents collaborate via ATN to build a CLI app:
1. Dev agent creates `app.py` with a `greet` command (via opencode + deepseek)
2. ATN routes a feature request to feature agent
3. Feature agent reads `app.py` and adds a `farewell` command
4. Both commands verified working; completion notice routed back

Run: `bash demo/run-demo.sh` (~55 seconds, ~$0.006 API cost)
Capture artifacts: `ATN_CAPTURE_DIR=demo/last-run bash demo/run-demo.sh`
Regression test: `REG_RS_DATA_DIR=work/reg-rs reg-rs run -p atn-demo`

## Replay Tooling

`atn-replay` renders PTY transcripts from `.atn/logs/*/transcript.log`:

```
atn-replay dashboard .atn/logs -o dashboard.org   # emacs auto-revert
atn-replay screenshot transcript.log              # text box to stdout
atn-replay screenshot transcript.log --html f.html # standalone HTML
atn-replay screenshot transcript.log --html-fragment f.html  # org embed
atn-replay list .atn/logs                         # list agents + sizes
```

## Logging

Three log files per agent in `.atn/logs/{agent_id}/`:
- `transcript.log` — raw PTY output bytes (replayable via atn-replay)
- `inputs.jsonl` — timestamped input commands (human_text, coordinator_command, etc.)
- `events.jsonl` — state transitions (prompt_ready, idle_detected, push_event)

## Architecture

Cargo workspace with 7 crates (library-first design):
- **atn-core**: pure domain types, no async, no I/O
- **atn-pty**: PTY session lifecycle via portable-pty + tokio
- **atn-server**: Axum binary with REST + SSE + embedded static UI
- **atn-wiki**: wiki storage and coordination logic
- **atn-trail**: agentrail file reader and CLI wrapper
- **atn-replay**: PTY transcript rendering (vt100 + clap)
- **atn-ui**: Yew components (placeholder)

## Known Issues

- ATN notification `(priority: High)` causes bash syntax error (unquoted parentheses)
- State tracking timing is fragile for programmatic task completion detection

## Upstream Projects

### agentrail-rs
**Location**: `~/github/sw-vibe-coding/agentrail-rs`
**Integration**: CLI binary in PATH, invoked by `atn-trail/src/cli.rs`

### wiki-rs
**Location**: `~/github/sw-vibe-coding/wiki-rs`
**Integration**: `wiki-common` crate used as path dependency (with `server` feature)

## Quality

- Zero clippy warnings (`cargo clippy --workspace -- -D warnings`)
- All tests passing (`cargo test --workspace`)
- Rust 2024 edition throughout
