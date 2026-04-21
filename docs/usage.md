# All Together Now — Usage Guide

## Prerequisites

- Rust 2024 edition (stable with edition = "2024" or nightly)
- macOS or Linux
- Optional: `claude` / `codex` / `opencode` / `gemini` / ... on PATH for the
  CLIs you want ATN to drive
- Optional: `mosh` and key-based `ssh` to any remote dev-user accounts you
  want to run agents on

See also:
- [Demo scripts](./demos-scripts.md) — every demo you can run against the current build, grouped by time + prereqs
- [Uber use-case](./uber-use-case.md) — the isolation-first multi-agent story
- [Three-agent demo walkthrough](./demo-three-agent.md)
- [Remote PTY manual test](./remote-pty.md)
- [Architecture](./architecture.md)
- [Project status](./status.md)

## Quick Start (empty-start)

ATN boots with zero agents. You compose them through the UI at runtime.

```bash
cargo build --workspace
cargo run -p atn-server          # uses agents.toml in cwd
# or
cargo run -p atn-server -- path/to/agents.toml
```

The default `agents.toml` in the repo has no `[[agent]]` entries:

```toml
[project]
name = "all-together-now"
log_dir = ".atn/logs"
```

That's the intended shape. Open http://localhost:7500 — you'll see an empty
dashboard with a **+ New Agent** call-to-action. Click it to add your first
agent.

> **Why no seed agents?** See [uber-use-case.md](./uber-use-case.md) — the
> design favors runtime composition and dev-user isolation on remote hosts
> over a static `agents.toml`. The legacy seed shape is preserved as
> `agents.example.toml` for reference.

## Creating Agents — The New Agent Dialog

The **+ New Agent** button opens a form that captures a structured spawn
specification. Fields:

| Field         | Required            | Example                                       |
|---------------|---------------------|-----------------------------------------------|
| `name`        | yes — unique id     | `worker-hlasm`                                |
| `role`        | default: `worker`   | `coordinator` \| `worker` \| `qa` \| `pm`     |
| `transport`   | default: `local`    | `local` \| `mosh` \| `ssh`                    |
| `user`        | iff transport≠local | `devh1`                                       |
| `host`        | iff transport≠local | `queenbee`                                    |
| `working_dir` | yes                 | `/home/devh1/work/hlasm` (path on the target) |
| `project`     | no (label only)     | `hlasm`                                       |
| `agent`       | yes                 | `claude` \| `codex` \| `opencode-z-ai-glm-5`  |
| `agent_args`  | no                  | `--resume --model sonnet`                     |

A **live preview** renders the composed command as you type:

- **local**: `cd <working_dir> && <agent> [agent_args]`
- **mosh / ssh**: `<mosh|ssh> <user>@<host> -- tmux new-session -A -s atn-<name> 'cd <working_dir> && <agent> [agent_args]'`

The `-A` flag on `tmux new-session` makes the remote session **idempotent**:
the first connect creates it, later reconnects re-attach. That's what makes
the **Reconnect** control survive network blips (see below).

Missing or malformed fields show up inline (`missing: host, user`) and the
**Create** button is disabled until the spec validates. Submission is a
`POST /api/agents` with the JSON payload — see the API table.

## Agent Lifecycle

| Control    | Endpoint                              | Effect                                                                                   |
|------------|---------------------------------------|------------------------------------------------------------------------------------------|
| Create     | `POST /api/agents`                    | Validates spec, composes the shell command, spawns the PTY.                              |
| Restart    | `POST /api/agents/{id}/restart`       | Graceful shutdown (Ctrl-C ×2, kill), then respawn. Sends `^C` to whatever is in the PTY. |
| Reconnect  | `POST /api/agents/{id}/reconnect`     | **Hard-kills** the local `mosh`/`ssh` child (no Ctrl-C), respawns. For mosh+tmux, this re-attaches to the still-running remote session so in-progress agent work survives. |
| Stop       | `POST /api/agents/{id}/stop`          | Shutdown without delete.                                                                 |
| Delete     | `DELETE /api/agents/{id}`             | For remote agents: first sends `^B :kill-session Enter` over the PTY so tmux cleans up server-side. Then the usual graceful shutdown.                                       |
| Update cfg | `PUT /api/agents/{id}`                | Edit and restart.                                                                        |

## Dashboard Views

### Agents (default)

Grid of agent panels, one per running agent. Each panel shows:

- **Header**: agent name + project label, role, saga step badge, state
- **Terminal**: live PTY output streamed via SSE (xterm.js)
- **Controls**: input box, Send, Ctrl-C, Restart, Reconnect, Stop, Delete

The grid layout adapts: 1/2/3/2×2 columns based on agent count.

### Graph

SVG dependency graph. Nodes color-coded by state; edges show blocking
relationships.

### Saga

`.agentrail/` progress with step cards, trajectories, and the "Distill Skill"
button for extracting skills from ICRL data.

### Wiki

Markdown wiki browser with page list, wiki-link navigation, and ETag-based
conflict detection. Also reachable standalone at `/wiki`.

### Events

Inter-agent event log. Two columns: outbound (coordinator→agents) on the
left, inbound (agents→coordinator) on the right. Escalation banners surface
at the top.

## Inter-Agent Communication

### Push Event Kinds

JSON events written to per-agent outboxes:

- `feature_request`, `bug_fix_request`, `completion_notice`,
  `blocked_notice`, `needs_info`, `verification_request`

### Routing

1. **Target known** → delivered to the target agent's inbox and injected as a
   prompt into the target's PTY.
2. **Target unknown** → escalated to the wiki `Coordination/Requests` page.
3. **No target** → broadcast to `Coordination/*`.

Events can be submitted via `POST /api/events`.

## Wiki Coordination Pages

ATN seeds these pages at startup:

| Page                      | Purpose                    |
|---------------------------|----------------------------|
| `Coordination/Goals`      | Project objectives         |
| `Coordination/Agents`     | Who is working on what     |
| `Coordination/Requests`   | Open requests between agents |
| `Coordination/Blockers`   | Dependency blockers         |
| `Coordination/Log`        | Timestamped event log       |

## Environment

| Variable            | Default       | Purpose                                                    |
|---------------------|---------------|------------------------------------------------------------|
| `ATN_PORT`          | `7500`        | Bind port for the HTTP/SSE server. `0` picks a free port.  |
| `RUST_LOG`          | `atn=info`    | Tracing filter.                                            |
| `ATN_DEMO_REAL`     | unset         | In `demos/three-agent/setup.sh`: `1` uses real agent CLIs instead of `tools/fake-*`. |
| `ATN_DEMO_SKIP_BOOT`| unset         | In `demos/three-agent/setup.sh`: `1` reuses a running server at `ATN_DEMO_URL`. |
| `ATN_DEMO_URL`      | `http://localhost:7500` | Base URL used by the demo script.                  |

When the server binds, it prints a machine-readable line:

```
atn-server ready on 0.0.0.0:<port>
```

Test harnesses (including `crates/atn-server/tests/three_agent_demo.rs`)
parse this to discover the OS-assigned port when `ATN_PORT=0`.

## Graceful Shutdown

SIGINT (Ctrl-C) and SIGTERM both:

1. Stop accepting new HTTP connections
2. Send Ctrl-C ×2 to each agent PTY
3. Force-kill any remaining child processes
4. Log status and exit cleanly

## Config Hot-Reload

The server watches `agents.toml` (debounced 500ms). Adding or removing
`[[agent]]` entries spawns / shuts down agents automatically. Agents created
via the New Agent dialog are **not** persisted to `agents.toml` unless you
click **Save** in the header (`POST /api/agents/save`).

## REST API Reference

| Method | Path                              | Description                              |
|--------|-----------------------------------|------------------------------------------|
| GET    | `/api/agents`                     | List agents with state, spec, launch_command |
| POST   | `/api/agents`                     | Create agent from a `SpawnSpec`          |
| PUT    | `/api/agents/{id}`                | Update agent config and restart          |
| DELETE | `/api/agents/{id}`                | Shut down and remove (tmux-aware)        |
| POST   | `/api/agents/{id}/restart`        | Ctrl-C + kill + respawn                  |
| POST   | `/api/agents/{id}/reconnect`      | Hard-kill local mosh/ssh + respawn (re-attach) |
| POST   | `/api/agents/{id}/stop`           | Shutdown without delete                  |
| GET    | `/api/agents/{id}/sse`            | SSE stream of terminal output            |
| POST   | `/api/agents/{id}/input`          | Send text or raw bytes                   |
| POST   | `/api/agents/{id}/ctrl-c`         | Shortcut for raw `0x03`                  |
| POST   | `/api/agents/{id}/resize`         | Update PTY rows/cols from the browser    |
| GET    | `/api/agents/{id}/state`          | State snapshot                           |
| POST   | `/api/agents/save`                | Persist runtime agents back to `agents.toml` |
| GET    | `/api/agents/graph`               | Dependency graph data                    |
| GET    | `/api/saga`                       | Project saga progress                    |
| POST   | `/api/saga/distill`               | Distill skills from trajectories         |
| GET    | `/api/agents/{id}/saga`           | Per-agent saga                           |
| GET    | `/api/events`                     | Event log (`?since=N`)                   |
| POST   | `/api/events`                     | Submit a `PushEvent`                     |
| GET    | `/api/wiki`                       | List wiki pages                          |
| GET    | `/api/wiki/{title}`               | Get page (JSON + ETag)                   |
| PUT    | `/api/wiki/{title}`               | Create/update page (`If-Match`)          |
| PATCH  | `/api/wiki/{title}`               | Structured patch ops                     |
| DELETE | `/api/wiki/{title}`               | Delete page (`If-Match`)                 |
| GET    | `/wiki`                           | Standalone wiki HTML                     |
| GET    | `/wiki/{title}`                   | Standalone wiki page                     |

## Logs & Transcripts

Per-agent files in `{log_dir}/{agent_id}/`:

- `transcript.log` — raw PTY bytes (replayable via `atn-replay`)
- `inputs.jsonl` — timestamped input events
- `events.jsonl` — structured signals (`prompt_ready`, `idle_detected`,
  `push_event`, `disconnected`)

## The Three-Agent Demo

Shortest path to seeing ATN coordinate multiple agents end-to-end:

```bash
# Runs against fake agent shims — no real CLIs required.
./demos/three-agent/setup.sh
```

See [docs/demo-three-agent.md](./demo-three-agent.md) for the full
walkthrough, including how to swap in real `claude` / `codex` /
`opencode-z-ai-glm-5` via `ATN_DEMO_REAL=1`.
