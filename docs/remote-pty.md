# Remote PTY — Manual Integration Walkthrough

This is the manual test for the remote-agent transport. CI uses `tools/fake-mosh`
to assert argv composition (`crates/atn-pty/tests/integration.rs::
remote_mosh_transport_records_expected_argv`); the walkthrough below exercises a
real `mosh` + `tmux` session to `queenbee` so a human can sign off on reconnect
and cleanup behavior. **Skipped in CI.**

## Prerequisites

- Mac with `mosh` and `ssh` clients installed and `ssh devh1@queenbee` already
  configured (key-based auth).
- `queenbee` has a dev user `devh1` whose home is the work tree for the
  HLASM project, and `tmux` installed.
- A fresh checkout of ATN with phase-3 (`003-remote-pty-transport`) changes
  in place.

## 1. Boot ATN empty

```
cargo run -p atn-server
```

Visit http://localhost:7500. Dashboard should show **0 agents** and an empty
state with a **+ New Agent** CTA.

## 2. Create one remote agent

Click **+ New Agent** and fill the dialog:

| Field        | Value                         |
|--------------|-------------------------------|
| name         | `worker-hlasm`                |
| role         | `worker`                      |
| transport    | `mosh`                        |
| user         | `devh1`                       |
| host         | `queenbee`                    |
| working_dir  | `/home/devh1/work/hlasm`      |
| agent        | `codex` (or `bash` for a smoke test) |

The live preview should read:

```
mosh devh1@queenbee -- tmux new-session -A -s atn-worker-hlasm \
  'cd /home/devh1/work/hlasm && codex'
```

Click **Create**. Server returns 201 with the composed `launch_command` echoed
back. A new panel appears in the dashboard streaming the remote TTY.

On `queenbee` in another terminal, confirm the session:

```
sudo -u devh1 tmux list-sessions
# atn-worker-hlasm: 1 windows (created ...) [120x40] (attached)
```

## 3. Simulate a network drop (reconnect re-attaches)

Break the mosh connection from the Mac side — e.g.

```
pkill -f 'mosh .* devh1@queenbee'
```

Expected:
- Dashboard panel's output stops.
- Agent state flips to **Disconnected** (visible in the header state chip).
- `tmux list-sessions` on `queenbee` still shows `atn-worker-hlasm` — the
  remote session survives.

Click **Reconnect** (or `POST /api/agents/worker-hlasm/reconnect`). ATN
hard-kills the local `mosh` child (without sending Ctrl-C, so nothing inside
tmux is interrupted) and respawns the composed command. Because the command
uses `tmux new-session -A -s atn-worker-hlasm`, the respawn **re-attaches** to
the still-running session and the agent's in-progress work is intact.

## 4. Destroy the agent (cleanup)

Click the agent's **Delete** control (or `DELETE /api/agents/worker-hlasm`).

ATN's delete path:
1. Reads the stored `SpawnSpec`. Remote → graceful remote shutdown.
2. Sends `Ctrl-B :kill-session <Enter>` over the PTY (raw bytes
   `\x02:kill-session\r`), waits 500ms.
3. Runs the usual local `PtySession::shutdown()` (Ctrl-C × 2, then kill).

Verify on `queenbee`:

```
sudo -u devh1 tmux list-sessions
# No server running on /tmp/tmux-1000/default
```

The remote tmux session should be gone.

## 5. Three-agent topology (scripted, step 4)

The full three-agent demo (coordinator on Mac + two queenbee workers using
different dev users and agent CLIs) is scripted in step `004-three-agent-demo`
and will live at `demos/three-agent/setup.sh`. That will stand up the exact
topology documented in `docs/uber-use-case.md`.

## Troubleshooting

- **Stays in `Starting`**: the outer shell hasn't typed the composed command
  yet. Check server logs — look for `Spawned agent: worker-hlasm`.
- **Immediate `Disconnected`**: the composed command failed at the outer bash
  (likely `mosh` not on PATH, or ssh-key prompts requiring interactive input).
  Verify `ssh devh1@queenbee true` works from the terminal first.
- **Reconnect respawns a fresh agent instead of re-attaching**: check the
  remote `tmux list-sessions` — if the session is gone, someone killed it.
  Reconnect in that case starts a new session (expected; tmux `-A` falls back
  to create-if-missing).
