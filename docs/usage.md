# All Together Now — Usage Guide

## Prerequisites

- Rust 2024 edition (nightly or stable with edition = "2024")
- Trunk (for WASM/Yew builds): `cargo install trunk`
- Claude Code installed and accessible in PATH
- macOS or Linux

## Quick Start

### 1. Build

```bash
# Build all crates
cargo build --workspace

# Build the WASM frontend
cd frontend && trunk build --release
```

### 2. Configure Agents

Create `agents.toml` in your project root:

```toml
[session]
wiki_dir = "./wiki"           # Where coordination wiki pages live
log_dir = "./logs"            # Transcript logs

[[agent]]
id = "frontend-dev"
name = "Frontend Developer"
repo_path = "/path/to/frontend-repo"
role = "developer"
setup_commands = [
    "sw project frontend",    # Your project env switcher
]
launch_command = "claude --dangerously-skip-permissions"

[[agent]]
id = "backend-dev"
name = "Backend Developer"
repo_path = "/path/to/backend-repo"
role = "developer"
setup_commands = [
    "sw project backend",
]
launch_command = "claude --dangerously-skip-permissions"

[[agent]]
id = "qa"
name = "QA Engineer"
repo_path = "/path/to/test-repo"
role = "qa"
setup_commands = []
launch_command = "claude --dangerously-skip-permissions"
```

### 3. Start the PGM

```bash
# Start with config file
atn-server --config agents.toml

# Or with defaults (looks for agents.toml in current dir)
atn-server
```

The server starts on `http://localhost:7500` by default.

### 4. Open the Dashboard

Navigate to `http://localhost:7500` in your browser.

You'll see a grid of agent panels, one per configured agent. Each panel
shows live terminal output, an input box, and action buttons.

## Working with Agents

### Sending Input

Type in the agent's input box and press Enter (or click Send). The text is
written to the agent's PTY followed by a newline.

### Interrupting an Agent

Click the **Ctrl-C** button. This sends `0x03` to the PTY, equivalent to
pressing Ctrl-C in a terminal. Claude Code requires two Ctrl-C presses to
quit entirely — one interrupts the current operation, two exits.

### The "claude go" Workflow

This matches the existing workflow where you interrupt Claude and tell it to
continue:

1. Click **Ctrl-C** on the agent panel.
2. Wait for output to settle (the button auto-waits).
3. Click **claude go** (or type `claude go` and Send).

The **claude go** button combines both steps with appropriate timing.

### Reading the Wiki

Click **Read Wiki** to inject a command that tells the agent to check
coordination pages. Equivalent to sending `coord inbox` if the agent has
the coordination shell helper installed.

## Wiki

### Viewing

Click **Wiki** in the navigation bar to browse coordination pages. The wiki
browser supports markdown rendering and wiki-links (`[[PageName]]`).

### Editing

Click Edit on any wiki page. Changes are saved with CAS (Compare-and-Swap)
protection — if another agent or the PGM modified the page since you loaded
it, you'll see a conflict notification.

### Coordination Pages

ATN seeds these pages automatically:

| Page | Purpose |
|------|---------|
| `Coordination/Goals` | What the team is trying to accomplish |
| `Coordination/Agents` | Who is working on what (auto-updated) |
| `Coordination/Requests` | Open feature/bug requests between agents |
| `Coordination/Blockers` | What's blocking whom |
| `Coordination/Log` | Append-only event log |

## Inter-Agent Communication

### How Agents Send Requests

Agents emit structured push events by writing a JSON file to a known
location or by outputting a specially formatted line. The PGM detects these
and routes them.

Push types:
- `feature_request` — "I need feature X from repo Y"
- `bug_fix_request` — "Bug #123 in repo Y is blocking me"
- `completion_notice` — "Feature X / fix for bug #123 is done"
- `blocked_notice` — "I'm blocked on dependency Z"
- `needs_info` — "I need more information about requirement W"
- `verification_request` — "Please verify fix for bug #123"

### How Routing Works

1. **Target known**: PGM injects the request directly into the target
   agent's PTY (waits for prompt-ready state).
2. **Target unknown**: PGM updates the wiki Requests page and optionally
   broadcasts a "read coordination page" nudge to all agents.
3. **Needs human decision**: PGM surfaces the request in the UI with a
   routing prompt.

## Workflow Tracking (Agentrail)

If an agent's repo has an `.agentrail/` directory, ATN integrates with it:

- The UI shows the current saga step for each agent.
- Step completions are automatically recorded as trajectories.
- On fresh context, skill docs and past successes are injected.

## Logs & Transcripts

All agent terminal output is logged to `{log_dir}/{agent_id}/transcript.log`.
Structured events are logged to `{log_dir}/events.jsonl`.

## Configuration Reference

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ATN_PORT` | `7500` | Server port |
| `ATN_CONFIG` | `agents.toml` | Config file path |
| `ATN_LOG_LEVEL` | `info` | Tracing log level |

### CLI Flags

```
atn-server [OPTIONS]

Options:
  -c, --config <PATH>    Agent config file [default: agents.toml]
  -p, --port <PORT>      Server port [default: 7500]
  -l, --log-dir <PATH>   Log directory [default: ./logs]
  -w, --wiki-dir <PATH>  Wiki directory [default: ./wiki]
      --no-wiki          Disable wiki integration
      --no-trail         Disable agentrail integration
```
