# All Together Now — Usage Guide

## Prerequisites

- Rust 2024 edition (nightly or stable with edition = "2024")
- Claude Code installed and accessible in PATH
- macOS or Linux

## Quick Start

### 1. Build

```bash
cargo build --workspace
```

### 2. Configure Agents

Create `agents.toml` in your project root:

```toml
[project]
name = "my-project"
log_dir = ".atn/logs"

[[agent]]
id = "frontend-dev"
name = "Frontend Developer"
repo_path = "/path/to/frontend-repo"
role = "developer"
setup_commands = [
    "nvm use 18",
]
launch_command = "claude --dangerously-skip-permissions"

[[agent]]
id = "backend-dev"
name = "Backend Developer"
repo_path = "/path/to/backend-repo"
role = "developer"
setup_commands = []
launch_command = "claude --dangerously-skip-permissions"

[[agent]]
id = "qa"
name = "QA Engineer"
repo_path = "/path/to/test-repo"
role = "qa"
setup_commands = []
launch_command = "claude --dangerously-skip-permissions"
```

### 3. Start the PGM Server

```bash
# Start with config file (default: agents.toml in current dir)
cargo run -p atn-server

# Or specify a config path
cargo run -p atn-server -- path/to/agents.toml
```

The server starts on `http://0.0.0.0:7500`.

### 4. Open the Dashboard

Navigate to `http://localhost:7500` in your browser.

## Dashboard Views

### Agents (default)

Grid of agent panels, one per configured agent. Each panel shows:
- **Header**: agent name, role, saga step badge, connection indicator, state badge
- **Terminal**: live PTY output streamed via SSE (xterm.js)
- **Controls**: input box, Send, Ctrl-C, and Restart buttons

The grid layout adapts: 1 column for 1 agent, 2 columns for 2, 3 columns for 3+.

### Graph

SVG dependency graph showing which agents are blocked and by what. Nodes are
color-coded by agent state. Edges show blocking relationships. Refresh to see
current state.

### Saga

Displays `.agentrail/` workflow progress. Shows saga metadata, step cards with
status badges, and ICRL trajectories for the current step. Includes a "Distill
Skill" button for extracting skills from trajectory data.

### Wiki

Coordination wiki browser with page list sidebar, markdown rendering, wiki-link
navigation, and ETag-based conflict detection for concurrent edits.

### Events

Push event log showing inter-agent communication. Escalation banners appear at
top when events need human attention. The Events tab highlights red when
escalations occur.

## Working with Agents

### Sending Input

Type in the agent's input box and press Enter (or click Send). The text is
written to the agent's PTY followed by a newline.

### Interrupting an Agent

Click **Ctrl-C** to send `0x03` to the PTY. Claude Code requires two Ctrl-C
presses to quit: one interrupts the current operation, two exits.

### Restarting an Agent

Click **Restart** to shut down the agent's PTY session (double Ctrl-C + kill)
and re-spawn it from the stored configuration. The terminal is cleared and SSE
reconnects automatically.

## Graceful Shutdown

The server handles SIGINT (Ctrl-C) and SIGTERM gracefully:
1. Stops accepting new HTTP connections
2. Sends Ctrl-C twice to each agent (1s apart)
3. Force-kills any remaining child processes
4. Logs shutdown status and exits cleanly

## Config Hot-Reload

The server watches `agents.toml` for changes. When the file is modified:
- **New agents** are spawned automatically
- **Removed agents** are shut down gracefully
- **Existing agents** have their stored configs updated (restart to apply)

Changes are debounced (500ms) to handle editor save patterns.

## Inter-Agent Communication

### Push Event Types

Agents communicate via structured push events written as JSON files:
- `feature_request` — "I need feature X from repo Y"
- `bug_fix_request` — "Bug #123 in repo Y is blocking me"
- `completion_notice` — "Feature X is done"
- `blocked_notice` — "I'm blocked on dependency Z"
- `needs_info` — "I need more info about requirement W"
- `verification_request` — "Please verify fix for bug #123"

### Routing Logic

1. **Target known**: delivered to the target agent's inbox and PTY notification
2. **Target unknown**: escalated to wiki Coordination pages + UI
3. **No target**: broadcast to wiki Coordination pages

Events can also be submitted via the REST API: `POST /api/events`

## Notification Sounds

The UI plays a short audio tone (Web Audio API, no external files) when:
- An agent transitions to **AwaitingHumanInput**, **Error**, or **Blocked** state
- An event escalation occurs (needs human attention)

## Wiki Coordination Pages

ATN seeds these pages automatically:

| Page | Purpose |
|------|---------|
| `Coordination/Goals` | Project objectives |
| `Coordination/Agents` | Who is working on what |
| `Coordination/Requests` | Open requests between agents |
| `Coordination/Blockers` | Dependency blockers |
| `Coordination/Log` | Timestamped event log |

## REST API Reference

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/agents` | List all agents with state |
| GET | `/api/agents/{id}/sse` | SSE stream of terminal output |
| POST | `/api/agents/{id}/input` | Send text input to agent |
| POST | `/api/agents/{id}/ctrl-c` | Send Ctrl-C to agent |
| GET | `/api/agents/{id}/state` | Get agent state |
| POST | `/api/agents/{id}/restart` | Restart agent session |
| GET | `/api/agents/graph` | Dependency graph data |
| GET | `/api/saga` | Project saga progress |
| POST | `/api/saga/distill` | Distill skills from trajectories |
| GET | `/api/agents/{id}/saga` | Agent-specific saga |
| GET | `/api/events` | Event log (`?since=N`) |
| POST | `/api/events` | Submit push event |
| GET | `/api/wiki` | List wiki pages |
| GET | `/api/wiki/{title}` | Get page (JSON + ETag) |
| PUT | `/api/wiki/{title}` | Create/update page (If-Match) |
| PATCH | `/api/wiki/{title}` | Patch page (structured ops) |
| DELETE | `/api/wiki/{title}` | Delete page (If-Match) |

## Logs & Transcripts

Agent terminal output is logged to `{log_dir}/{agent_id}/transcript.log`.
Structured events are logged to `{log_dir}/{agent_id}/events.jsonl`.

## Environment

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `atn=info` | Tracing log level filter |
