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

## Implementation Notes (for follow-up work)

- Empty-start: `atn-server` must come up cleanly with no agents registered;
  the UI must render the empty state and surface the **New Agent** affordance.
- New Agent dialog: form fields above, validated client-side; POST to a
  new `POST /agents` endpoint that spawns the PTY and returns the new agent's
  id + initial state.
- Command field is a free-form shell command so users can compose
  `mosh ... -- tmux ... -- cd ... && <agent-cli>` without ATN needing to
  understand mosh or tmux.
- Remote `cwd` is advisory (the command itself does the `cd`); local `cwd`
  is applied before spawn.
- Graceful removal: closing an agent should terminate the PTY cleanly (mosh
  + tmux survives the mosh disconnect on the server side, which is the point
  -- reconnecting re-attaches to the same tmux session and the agent CLI
  keeps running).
