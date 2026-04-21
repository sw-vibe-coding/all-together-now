# Three-Agent Demo Walkthrough

> Looking for other demos? See [docs/demos-scripts.md](./demos-scripts.md)
> for the full menu.

End-to-end demonstration of the topology described in
[docs/uber-use-case.md](./uber-use-case.md): a local coordinator on the Mac
talking to two isolated remote workers on a rack server, each owning a
different repo and running a different agent CLI.

| Agent         | Transport | user    | host        | dir                     | CLI                   |
|---------------|-----------|---------|-------------|-------------------------|-----------------------|
| coordinator   | local     | —       | mighty-mike | `~/work/atn-demo`       | `claude`              |
| worker-hlasm  | mosh      | `devh1` | queenbee    | `/home/devh1/work/hlasm`| `codex`               |
| worker-rpg    | mosh      | `devr1` | queenbee    | `/home/devr1/work/rpg-ii`| `opencode-z-ai-glm-5` |

There are **two paths** through this demo:

1. **CI / fake** — runs anywhere, no rack server required. Uses the three
   shell-script shims in `tools/` in place of real agent CLIs.
2. **Live** — requires `queenbee` to be reachable over `mosh` with `devh1`
   and `devr1` configured, plus real `claude` / `codex` /
   `opencode-z-ai-glm-5` installed.

## Path 1 — CI (fake agents)

### Automated integration test

```bash
cargo test -p atn-server --test three_agent_demo -- --nocapture
```

This test:

1. Writes an empty `agents.toml` into a tempdir.
2. Spawns the real `atn-server` binary with `ATN_PORT=0` (OS-picks) and
   `tools/` prepended to `PATH`.
3. Parses `atn-server ready on 0.0.0.0:<port>` off stdout.
4. `GET /api/agents` → `[]` (empty start).
5. POSTs three `SpawnSpec` payloads (local transport, fake CLIs).
6. Polls `/api/agents` until all three are out of `starting`.
7. POSTs two `PushEvent`s (coordinator → worker-hlasm, coordinator →
   worker-rpg) and waits for the router to deliver both.
8. Asserts both inboxes received a file and `/api/events` lists both.
9. Kills the server on drop.

Expected: green in a few seconds. This is the CI path; it's what guards the
three-agent topology from regressions.

### Interactive fake-agent demo

To watch it live in the browser without the test harness:

```bash
./demos/three-agent/setup.sh
```

The script:

1. Launches `atn-server` (or reuses whatever is at
   `$ATN_DEMO_URL`, default `http://localhost:7500`).
2. Waits for `/api/agents` to respond.
3. POSTs each fixture from `demos/three-agent/fixtures/` in order:
   `coordinator.json` → `worker-hlasm.json` → `worker-rpg.json`.

Open http://localhost:7500. With the default `$PATH` prepended with the
repo's `tools/` dir, the three agent fields resolve to:

```
coordinator    → tools/fake-claude
worker-hlasm   → tools/fake-codex
worker-rpg     → tools/fake-opencode-glm5
```

What you should see in the dashboard:

```
┌────────────────────┬────────────────────┐
│ coordinator        │ worker-hlasm       │
│ (atn-demo)         │ (hlasm)            │
│ role: coordinator  │ role: worker       │
│ ──────────────────│ ──────────────────│
│ fake-claude: ...   │ fake-codex: ...    │
│ claude> _          │ codex> _           │
├────────────────────┼────────────────────┤
│ worker-rpg         │ [+ New Agent] btn  │
│ (rpg-ii)           │                    │
│ role: worker       │                    │
│ ──────────────────│                    │
│ fake-opencode...   │                    │
│ glm5> _            │                    │
└────────────────────┴────────────────────┘
```

Each fake shim prints an identifying banner at spawn, then loops echoing
stdin to stdout with a per-agent prefix (`claude>`, `codex>`, `glm5>`). You
can type into a panel, press Send, and see the echo.

Route a message from the coordinator to a worker via the **Events** tab or
the API:

```bash
curl -X POST -H 'Content-Type: application/json' \
     -d '{
       "id":"demo-1",
       "kind":"feature_request",
       "source_agent":"coordinator",
       "source_repo":".",
       "target_agent":"worker-hlasm",
       "summary":"please add the foo/bar glue",
       "priority":"normal",
       "timestamp":"2026-04-18T18:00:00Z"
     }' \
     http://localhost:7500/api/events
```

Within the 2 s router poll cycle a new `[ATN task from coordinator]` line
shows up in `worker-hlasm`'s terminal and an entry appears in the Events
log.

## Path 2 — Live (real agents on queenbee)

### Prereqs

On the Mac:
- `claude` on PATH (or whatever you want the local coordinator to run)
- `mosh` client
- `ssh devh1@queenbee` and `ssh devr1@queenbee` work with keys

On queenbee:
- `tmux` installed
- user `devh1` with `/home/devh1/work/hlasm` checked out r/w, other repos
  r/o as needed
- user `devr1` with `/home/devr1/work/rpg-ii` checked out r/w
- the matching agent CLIs (`codex` for devh1, `opencode-z-ai-glm-5` for
  devr1) installed **for that dev user**

### Run the demo

```bash
ATN_DEMO_REAL=1 ./demos/three-agent/setup.sh
```

With `ATN_DEMO_REAL=1` the script **does not** prepend `tools/` to `PATH`,
so the three fixtures' `agent` fields resolve to whatever's actually on
`PATH` for each dev user.

What ATN will do for each fixture:

- **coordinator** (`transport: local`) — types into the local PTY:
  `cd ~/work/atn-demo && claude`
- **worker-hlasm** (`transport: mosh`) — types into the local PTY:
  `mosh devh1@queenbee -- tmux new-session -A -s atn-worker-hlasm 'cd /home/devh1/work/hlasm && codex'`
- **worker-rpg** (`transport: mosh`) — types:
  `mosh devr1@queenbee -- tmux new-session -A -s atn-worker-rpg 'cd /home/devr1/work/rpg-ii && opencode-z-ai-glm-5'`

On queenbee after setup you can verify the tmux sessions:

```
sudo -u devh1 tmux list-sessions  # atn-worker-hlasm
sudo -u devr1 tmux list-sessions  # atn-worker-rpg
```

### Reconnect on network blip

Kill your Mac's `mosh` processes to simulate a drop:

```
pkill -f 'mosh devh1@queenbee'
```

The `worker-hlasm` panel stops streaming and its state flips to
**Disconnected**. The remote tmux session **survives** — `codex` is still
running inside it on queenbee.

Click **Reconnect** (or `POST /api/agents/worker-hlasm/reconnect`). ATN
hard-kills the dead local mosh child and respawns the composed command.
Because it uses `tmux new-session -A -s atn-worker-hlasm`, the respawn
**re-attaches** to the still-running session and you see codex's in-progress
output reappear instantly.

See [docs/remote-pty.md](./remote-pty.md) for the deeper manual protocol
and troubleshooting.

### Clean teardown

Click **Delete** on any remote agent. ATN first sends
`^B :kill-session <Enter>` over the PTY (so tmux cleans up the session on
queenbee), then runs the usual local shutdown.

## Swapping the agent mix

The fixtures in `demos/three-agent/fixtures/` are just JSON files. Change
`agent`, `user`, `host`, or `working_dir` to point at any mix of agent CLIs
and dev users you like — the server composes the shell command the same way
(see [docs/usage.md § Creating Agents](./usage.md#creating-agents--the-new-agent-dialog)).

## What this demo proves

- **Empty start** works — no seed `[[agent]]` entries in `agents.toml`.
- **Structured spawn** composes correct commands for both local and
  mosh/ssh transports. `crates/atn-core/src/spawn_spec.rs` unit tests assert
  byte-for-byte composition; `crates/atn-pty/tests/integration.rs` asserts
  actual argv via `tools/fake-mosh`.
- **Router delivery** crosses all three agents (see the CI test assertions).
- **Reconnect** re-attaches the remote tmux after a simulated network drop
  (manual test in `docs/remote-pty.md`).
- **Graceful delete** cleans up the remote tmux session.

Together that's the full isolation-per-agent story from
[docs/uber-use-case.md](./uber-use-case.md), stood up from an empty ATN in a
few minutes.
