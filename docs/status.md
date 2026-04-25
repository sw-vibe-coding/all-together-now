# All Together Now — Status

## Current State: Git-Sync-Agents Saga Complete

New per-agent daemon `atn-syncd` watches each agent's worktree
for a `.atn-ready-to-pr` marker; when one shows up, it pushes the
named branch as `refs/heads/pr/<agent>-<branch>` to a configured
central remote and writes a `PrRecord` JSON into `<prs-dir>/<id>.json`.
`atn-server` exposes the registry via `GET /api/prs` (with
`?status=open|merged|rejected` filter), `GET /api/prs/{id}`,
`POST /api/prs/{id}/merge` (runs `git merge --no-ff` on
`--central-repo`; flips `status` + stamps `merge_commit` /
`merged_at`; 409 on conflict with `{error, stderr}` + auto
`merge --abort`), and `POST /api/prs/{id}/reject`. `atn-cli prs
{list, show, merge, reject}` drives the surface from the
terminal. End-to-end demo: two agent worktrees + one bare
central + one atn-server + per-agent atn-syncd processes — both
PRs land merge commits on central main without humans copy-
pasting diffs. 30 unit tests across atn-syncd / atn-server::prs /
atn-cli + 4 integration tests (atn-syncd binary fixture,
`/api/prs` HTTP, atn-cli prs round-trip, two-agent demo). See
[docs/git-sync-agents.md](./git-sync-agents.md) and
[demos-scripts.md § Demo 13](./demos-scripts.md#demo-13--git-sync-agents-end-to-end).

## Prior Milestone: atn-agent Saga Complete

New Rust-native agent wrapper `atn-agent` runs as an ATN
`launch_command`, polls `<atn-dir>/inboxes/<id>/` for messages,
and drives an Ollama-compatible `/api/chat` tool-calling loop.
Five tools (`file_read`, `file_write`, `shell_exec`,
`outbox_send`, `inbox_ack`) back the standard agent workflow:
read/write workspace files in a sandbox, run shell behind an
`--allow-shell` gate with timeout + output cap, emit `PushEvent`s
into the router's outbox, and explicitly ack inbox messages.
39 unit tests + 2 end-to-end integration tests (stubbed
`/api/chat` server, no Ollama required). See
[docs/atn-agent.md](./atn-agent.md) and
[demos-scripts.md § Demo 12](./demos-scripts.md#demo-12--atn-agent-end-to-end).

## Prior Milestone: Dashboard-Polish Saga Complete

Events view now has a client-side filter bar (text search + kind
chips + delivered radio + `K / N entries` counter) and every event
card click-expands to a full JSON detail panel. Escalation banners
sprout a `jump to event ▸` link. The new **📖 Wiki panel** is a
right-edge drawer that mirrors any wiki page beside the dashboard,
ETag-polls every 5 s (with server-side `If-None-Match` → 304), and
flashes on real changes. Event rows with a `wiki_link` reuse the
open panel instead of opening a new tab. See
[docs/events-view.md](./events-view.md) and
[docs/windowed-ui.md § Wiki side panel](./windowed-ui.md#wiki-side-panel).
Scripted walkthrough: [demos-scripts.md § Demo 11](./demos-scripts.md#demo-11--events-view--wiki-panel).

## Prior Milestone: atn-cli Saga Complete

Typed HTTP client `atn-cli` now wraps every REST endpoint the UI
uses — agents lifecycle + observation, events list + send, wiki
list/get/put/delete with ETag round-trips. Replaces the `curl +
jq` loops that were accumulating across the demo scripts. See
[docs/atn-cli.md](./atn-cli.md) for the full reference and
[demos-scripts.md § Demo 10](./demos-scripts.md#demo-10--atn-cli-tour)
for a scripted walkthrough.

## Prior Milestone: Windowed-UI Saga Complete

The dashboard now ships a desktop-window-manager model (Tiled / Stack
/ Carousel + per-window chrome + keyboard Option C). The heat-sized
treemap (scale-UI saga) is preserved as the legacy model for
large-fleet use — see
[docs/windowed-ui.md](./windowed-ui.md) for the new model and
[docs/scale-ui.md](./scale-ui.md) for the legacy walkthrough.

## Prior Milestone: Remote-Agent Demo Saga Complete

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
| `atn-cli` | Complete | Typed HTTP client for every REST endpoint (agents / events / wiki) |
| `atn-agent` | Complete | Rust-native AI-agent wrapper (Ollama `/api/chat` + 5 tools) |
| `atn-syncd` | Complete | Out-of-band git-sync daemon (marker → push → `PrRecord`) |
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
| S7 | Search + chips + group-by + saved layouts | Done |
| S8 | Scale-UI fleet (21 fake agents) + docs/scale-ui.md walkthrough | Done |
| W1 | Windowed-UI tiled foundation: strip treemap/squarify; coord-left + workers grid | Done |
| W2 | Window chrome: per-panel min/max/pin/config/reconnect/delete + click-to-select + bottom dock | Done |
| W3 | Stack layout: one primary at ~80% + dock of minimized; enter/leave reconcile state | Done |
| W4 | Carousel layout: primary + prev/next peeks + `◀/▶` cycle (excludes minimized) | Done |
| W5 | Keyboard Option C: `m`/`M`/`p`/`←→`/`1..9`/`Esc` on selected, guarded by xterm focus | Done |
| W6 | Persistence (`atn-window-ui-v1`) + sort selector + `docs/windowed-ui.md` + `demos/windowed-ui/setup.sh` + Demo 9 + scale-ui.md legacy banner | Done |
| O1 | Ops-polish: shell-escape CannedAction page + request_id (fixes `(priority: High)` bash bug) | Done |
| O2 | Ops-polish: `atn_pty::snapshot` vt100 renderer (text / ANSI / HTML) | Done |
| O3 | Ops-polish: `GET /api/agents/{id}/screenshot` + router flake fix | Done |
| O4 | Ops-polish: 📸 snapshot button in the window chrome | Done |
| O5 | Ops-polish: per-agent output-stall watchdog + `stalled` / `stalled_for_secs` in state | Done |
| O6 | Ops-polish: watchdog actions — Ctrl-C + `blocked_notice` + amber pulsing UI badge | Done |
| C1 | atn-cli scaffold: crate + clap + ureq + `agents list`/`state` | Done |
| C2 | atn-cli: `agents input`/`stop`/`restart`/`reconnect`/`delete`/`wait`/`screenshot` | Done |
| C3 | atn-cli: `events list` + `events send` | Done |
| C4 | atn-cli: `wiki list`/`get`/`put`/`delete` with ETag handling | Done |
| C5 | atn-cli: integration test + `docs/atn-cli.md` + Demo 10 + C1..C5 status rows | Done |
| D1 | Dashboard polish: Events view filter chips + text search + delivered toggle | Done |
| D2 | Dashboard polish: inline event-row expand + escalation `jump to event ▸` | Done |
| D3 | Dashboard polish: global wiki side-panel (read-only) with page picker + persistence | Done |
| D4 | Dashboard polish: wiki panel live updates (ETag / 304 / flash) + events-row cross-link | Done |
| D5 | Dashboard polish: `docs/events-view.md` + windowed-ui.md wiki panel section + Demo 11 + D1..D5 | Done |
| A1 | atn-agent scaffold: crate + clap CLI + banner + inbox poll + .json.done rename | Done |
| A2 | atn-agent: Ollama `/api/chat` integration (typed shapes + 60 s timeout + URL-echoing errors) | Done |
| A3 | atn-agent: `file_read` + `file_write` tools + path sandboxing + tool-call dispatch loop | Done |
| A4 | atn-agent: `shell_exec` (gated, 30 s timeout, 4 KiB cap) + `outbox_send` + `inbox_ack` tools | Done |
| A5 | atn-agent: integration test (stub HTTP) + `docs/atn-agent.md` + Demo 12 + A1..A5 status rows | Done |
| G1 | atn-syncd scaffold + marker detection: clap CLI + poll loop + atn-core::pr | Done |
| G2 | atn-syncd: marker parser + `git push <remote> <branch>:refs/heads/pr/<agent>-<branch>` + `PrRecord` JSON write + queued-marker rename | Done |
| G3 | atn-server: `/api/prs` REST surface (list / show / merge / reject) + `--prs-dir` / `--central-repo` flags | Done |
| G4 | atn-cli: `prs {list, show, merge, reject}` against the new REST + 5-col table + 409 stderr surfacing | Done |
| G5 | atn-syncd binary integration test + `docs/git-sync-agents.md` + `demos/git-sync/setup.sh` + Demo 13 + G1..G5 status rows | Done |

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

Cargo workspace with 10 crates (library-first design):
- **atn-core**: pure domain types, no async, no I/O
- **atn-pty**: PTY session lifecycle via portable-pty + tokio
- **atn-server**: Axum binary with REST + SSE + embedded static UI
- **atn-wiki**: wiki storage and coordination logic
- **atn-trail**: agentrail file reader and CLI wrapper
- **atn-replay**: PTY transcript rendering (vt100 + clap)
- **atn-cli**: typed HTTP client for the REST API (clap + ureq)
- **atn-agent**: Rust-native AI-agent wrapper (clap + ureq + Ollama /api/chat)
- **atn-syncd**: out-of-band git-sync daemon (marker → push → `PrRecord`)
- **atn-ui**: Yew components (placeholder)

## Known Issues

_None tracked at present._

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
