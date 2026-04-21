# Uber Use-Case: Distributed Multi-Agent Coordination via ATN

## TL;DR

Start ATN with **zero agents** -- just the web UI, wiki, and message queues. Add
agents one at a time through the UI. Each agent is a PTY owned by ATN that
typically runs `mosh` into a remote dev-user account where a real agent CLI
(Claude, Codex, OpenCode, Gemini, ...) is started inside `tmux`, scoped to a
single repository it owns read/write. Coordination happens entirely through
ATN's event queues and shared wiki; cross-repo changes flow back to GitHub via
out-of-band sync agents that act as a "pull request" equivalent.

The point is **hard isolation between agents** (no agent can reach into another
agent's repo and make uncoordinated changes) combined with a **single pane of
glass** for every agent's prompt, event queue, wiki contributions, and overall
progress.

## Motivation

Today when multiple agent sessions share a filesystem they can and do step on
each other: agent A edits files in agent B's repo, agent B overwrites them,
commits interleave, history gets tangled. Review becomes impossible. The
current workarounds (worktrees, branch discipline) rely on the agents behaving,
which is not a safety property.

The fix is structural: give each agent its own Unix user on its own machine
with filesystem permissions that make cross-repo writes physically impossible.
All shared state is explicit and goes through ATN.

## Topology

```
  Mac (laptop, runs ATN server + UI)
  ├── PTY: coordinator  -> claude (local)  working in manager/~/projects/plan-repo
  │
  ├── PTY: worker-hlasm -> mosh devh1@queenbee  -> tmux -> codex
  │                        /home/devh1/work/hlasm-repo
  │
  ├── PTY: worker-rpg   -> mosh devr1@queenbee  -> tmux -> opencode (zai-glm5)
  │                        /home/devr1/work/rpg-ii-repo
  │
  └── PTY: worker-ocaml -> mosh devo1@swarm     -> tmux -> gemini
                           /home/devo1/work/ocaml-repo

  Out-of-band sync agents (not owned by ATN PTY):
    - watch each dev user's repo for "ready to PR" markers
    - push branches to a central git remote
    - open PR-equivalents using local tooling (not github.com/pulls directly)
    - mirror merged changes back to the r/o clones on peer dev users
```

### Roles

- **Mac host**: runs `atn-server` (HTTP + SSE), the Yew UI, the wiki store,
  and any purely local agents (e.g. the coordinator).
- **Rack servers** (`queenbee`, `swarm`, ...): Arch Linux, each with several
  dedicated dev users. A dev user is the smallest unit of isolation.
- **Dev user**: owns exactly one repo r/w in its home directory; has r/o
  clones of other repos it needs to read. Enforced by Unix permissions, not
  by convention.
- **Agent CLI**: whatever the user picks per worker -- Claude, Codex,
  OpenCode + GLM-5, Gemini, etc. ATN doesn't care; it just owns the PTY.
- **Sync agents**: out-of-band processes (not ATN PTYs) that move git state
  between dev users and GitHub.

## Starting from Empty

ATN boots with:

- web UI up
- wiki available
- message queues empty
- **zero** agent PTYs

From the UI the user clicks **New Agent** and a dialog collects:

| Field         | Example                                                    |
|---------------|------------------------------------------------------------|
| Agent name    | `worker-hlasm`                                             |
| Role          | `coordinator` \| `worker`                                  |
| Working dir   | `/home/devh1/work/hlasm-repo` (remote) or local path       |
| Command(s)    | `mosh devh1@queenbee -- tmux new-session -A -s atn 'cd /home/devh1/work/hlasm-repo && codex'` |
| Env / secrets | (optional)                                                 |

ATN spawns a PTY running that command, attaches its stdio to the event plane,
and registers the agent with the router. From that point on the agent shows up
in the UI grid, the Events view, the wiki participant list, and every other
ATN surface.

Adding and removing agents is a runtime operation. No server restart, no
config-file edit.

## Demo Scenario

Goal: make a coordinated change that spans the HLASM repo, the RPG II repo,
and the OCaml repo -- e.g. introduce a shared message format and update each
codebase's reader/writer for it.

1. Start ATN on the Mac. No agents yet.
2. Add `coordinator` (local `claude`, Mac, pointing at a small plan repo).
3. Coordinator drafts the plan on the wiki, posts tasks to three event queues.
4. Add `worker-hlasm`, `worker-rpg`, `worker-ocaml` one by one. Each connects
   via mosh into its dedicated dev user on the assigned server, drops into
   tmux, and launches its chosen agent CLI. Each sees only its own r/w repo;
   peer repos are r/o if they need to read them at all.
5. Workers pull tasks from their queues, read shared context from the wiki,
   do work in their own repo, and push status events back.
6. Out-of-band sync agents watch each dev user's repo for a "ready" marker,
   push branches to a central remote, and open PR-equivalents using the local
   tooling. Merges propagate back to the r/o clones on peer dev users.
7. Coordinator watches the queues + wiki, sequences follow-up tasks, and
   eventually marks the goal done.

Throughout, the human sits in front of one ATN web UI and sees: every PTY's
live output, the event queues, the shared wiki, and per-agent agentrail state.

## What ATN Owns vs Doesn't

**ATN owns**:

- the PTY for each agent (stdin serialized, stdout streamed to UI + event bus)
- the event queues (per-agent inbox/outbox)
- the shared wiki
- the single-pane UI
- per-agent agentrail trajectories

**ATN does not own**:

- what runs inside the PTY (any agent CLI works)
- how the remote shell gets there (mosh, ssh, tmux -- all user-configured)
- git sync between dev users and GitHub (out-of-band sync agents do this)
- filesystem permissions (Unix users + mode bits on the servers do this)

This keeps ATN small and lets the isolation story rest on boring,
well-understood primitives (Unix users, ssh/mosh, tmux, git) rather than on
anything ATN has to enforce itself.

## Why This Matters

- **Isolation is structural, not behavioral.** An agent cannot touch another
  agent's repo because the OS won't let it. No prompt discipline required.
- **Single view.** Every prompt, every event, every wiki edit, every
  trajectory is visible in one UI, regardless of which server the agent is
  running on.
- **Heterogeneous agents.** Mix Claude, Codex, OpenCode, Gemini freely;
  pick the right tool per repo/language.
- **All state eventually flows to GitHub.** The PR-equivalent workflow means
  every accepted change has a normal reviewable git history, even though the
  work happened across many isolated dev users.
- **Runtime composition.** Start empty, add agents as the work demands, tear
  them down when done. No static cluster config.

## Implementation Notes (what now exists)

This section was originally a sketch for follow-up work; it now reflects
what the remote-agent demo saga actually landed.

- **Empty-start** — `atn-server` boots cleanly with zero agents. The
  shipped `agents.toml` has only `[project]`; the legacy seed is preserved
  as `agents.example.toml` for reference. The Yew UI and the static HTML
  dashboard both render a deliberate empty state with a **+ New Agent**
  call-to-action. See `docs/usage.md § Quick Start`.
- **Structured New Agent dialog** — instead of a free-form command field,
  the dialog captures a `SpawnSpec` (name, role, transport ∈ local/mosh/
  ssh, host, user, working_dir, project, agent, agent_args). The server
  composes the shell command from those parts:
  - local: `cd <working_dir> && <agent> [agent_args]`
  - mosh/ssh: `<bin> <user>@<host> -- tmux new-session -A -s atn-<name>
    'cd <working_dir> && <agent> [agent_args]'`
  Validation rejects missing fields per transport and injection-prone
  characters. Implementation: `crates/atn-core/src/spawn_spec.rs`. The
  POST endpoint is `POST /api/agents`; see
  `docs/usage.md § REST API Reference` for the full surface.
- **Reconnect after network drop** — `POST /api/agents/{id}/reconnect`
  hard-kills the local mosh/ssh child without sending Ctrl-C, then
  respawns the same composed command. Because the mosh/ssh template uses
  `tmux new-session -A -s atn-<name>`, the respawn re-attaches to the
  still-running remote tmux session and in-progress agent work survives.
  PTY exit detection flips state to `Disconnected` via a new
  `OutputSignal::Disconnected` signal. Manual walkthrough:
  `docs/remote-pty.md`.
- **Graceful remote delete** — `DELETE /api/agents/{id}` consults the
  stored `SpawnSpec`; for mosh/ssh agents it sends `^B :kill-session
  <Enter>` over the PTY before the usual shutdown, so the remote tmux
  session is cleaned up server-side.
- **Three-agent topology as CI** — `crates/atn-server/tests/
  three_agent_demo.rs` spawns the real binary on an ephemeral
  (`ATN_PORT=0`) port with `tools/` on PATH, POSTs three local-variant
  SpawnSpecs backed by `tools/fake-{claude,codex,opencode-glm5}`, and
  asserts router delivery of coordinator→worker events. The real
  (mosh-to-queenbee) topology lives in `demos/three-agent/fixtures/*.json`
  and is driven by `demos/three-agent/setup.sh`
  (`ATN_DEMO_REAL=1` to flip from fake shims to real CLIs).

## See also

- [docs/demos-scripts.md](./demos-scripts.md) — curated demo menu
  (empty-start, three-agent, reconnect, treemap scale-UI, REST tour)
  with setup / steps / what each one shows.
- [docs/usage.md](./usage.md) — operational guide: empty-start,
  New Agent dialog, REST surface, environment variables.
- [docs/demo-three-agent.md](./demo-three-agent.md) — end-to-end
  walkthrough of the exact topology described above.
- [docs/remote-pty.md](./remote-pty.md) — manual test for real mosh+tmux
  sessions to queenbee, reconnect, and cleanup.
- [docs/status.md](./status.md) — project status, what's shipped.
- [docs/architecture.md](./architecture.md) — crate layout and design.
