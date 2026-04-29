# UI Walkthrough

End-to-end UI test that grows the agent topology incrementally and
verifies every dashboard view (Agents, Graph, Saga, Wiki, Events)
updates correctly at each step.

The terminal scenario is a Factorio-style production chain:
**coordinator** with goal `2 gadgets + 2 gizmos`; resource gatherers,
smelter, stamper, assembler. Every Done message updates a wiki
inventory page; coordinator reports rolled-up status.

The walkthrough doubles as a **regression test for the dashboard**.
If a phase's expectations diverge from observed state, that's a
dashboard bug, not a demo bug.

## Setup

Pre-conditions for a clean run:

```bash
# 1. Kill any running atn-server (force if needed; it tends to spin
#    at >100% CPU when wedged and ignores SIGTERM)
pkill -9 -f 'target/debug/atn-server' || true

# 2. Wipe per-agent state from prior sessions
rm -rf .atn/logs/{coordinator,gatherer-iron,gatherer-coal,smelter,stamper,assembler}
rm -rf .atn/inboxes .atn/outboxes

# 3. Optional — also wipe wiki to start truly empty.
#    Without this, 5 Coordination__* pages from prior sessions persist.
#    rm -rf .atn/wiki

# 4. Boot a fresh server (no agents, no auto-load)
./target/debug/atn-server agents.toml &

# 5. Open localhost:7500 in a browser; in DevTools console:
#    localStorage.clear(); location.reload();
```

## API + DOM probe shapes (reference)

| Surface | Endpoint / Selector | Empty | One coordinator |
|---------|---------------------|-------|-----------------|
| Agents API | `GET /api/agents` | `[]` | `[{id: "coordinator", ...}]` |
| Graph API | `GET /api/agents/graph` | `[]` | `[{id, role, state, blocked_on: []}]` |
| Events API | `GET /api/events` | `[]` (or `{events: []}`) | same |
| Wiki API | `GET /api/wiki` | `[]` (titles array — but **persists across server restarts**) | unchanged |
| Agents DOM | `.agent-panel` | 0 | 1 (`panel-<id>`) |
| Sparkline DOM | `.spark-cell` | 0 | 1 |
| Graph DOM | `g.graph-node` (under `#graph-view svg`) | 0 | 1 |
| Saga DOM | `#saga-view` text | "No agent sagas found." | unchanged unless agent dir has `.agentrail/` |
| Events DOM | `#events-view .event-row` | 0 | 0 |

**Wiki gotcha:** the wiki backing dir survives server restart. Either
nuke `.atn/wiki/` for a true empty start, or accept the 5 pre-existing
`Coordination__*` titles in baseline expectations.

**Saga gotcha:** the Saga tab requires each agent to have a
`.agentrail/` dir in its `repo_path`. Most demo workers don't, so the
Saga tab stays "No sagas" through the whole walkthrough unless we
seed `.agentrail/` per agent (out of scope for v1 of this doc).

## Phase table

Phases are independent steps. After each, hard-reload the dashboard
(`location.reload()` in DevTools) to make probes deterministic — the
running tab's SSE merges new state in but timing is fuzzy for tests.

**Graph edge semantics** (settled in Phase 2): the dashboard always
draws `coordinator → worker` edges as a fan-out for every non-coord
agent (`static/index.html:4431-4438`), independent of `blocked_on`
or any event traffic. Real `blocked_on` edges are drawn on top.
Net: `edges = (N - 1) + sum(blocked_on across nodes)`. Phase 3's
`feature_request` will NOT add an edge — it adds events.

| # | Action | Agents | Graph (nodes / edges) | Events | Wiki |
|---|--------|--------|----------------------|--------|------|
| 0 | (empty server) | 0 panels, "0 agents" | "No agents running." | 0 | (5 carry-over titles) |
| 1 | POST coordinator | 1 panel | 1 / 0 | 0 | 5 |
| 2 | POST gatherer-iron | 2 panels | 2 / 1 (fan-out) | 0 | 5 |
| 3 | Coord → gatherer-iron `feature_request` | 2 panels | 2 / 1 (no change) | 1 (fwd) | 5 (+1 if Production__Goals seeded) |
| 4 | gatherer-iron Done | 2 panels | 2 / 1 | 2 (fwd + back) | unchanged |
| 5 | POST gatherer-coal | 3 panels | 3 / 2 | 2 | unchanged |
| 6 | POST smelter | 4 panels | 4 / 3 | 2 | unchanged |
| 7 | POST stamper | 5 panels | 5 / 4 | 2 | unchanged |
| 8 | POST assembler | 6 panels | 6 / 5 | 2 | unchanged |
| 9 | Run full goal: 2 gadgets + 2 gizmos | 6 panels (heat-spread, treemap demotes some to compact mid-run) | 6 / 5 baseline + transient `blocked_on` edges if any worker stalls | many (8-10 fwd + 5 Done + possible blocked_notice) | 5 + 1 (`Production__Goals` seeded by coord) |

## Phase status

- [x] Phase 0 — empty
- [x] Phase 1 — coordinator only
- [x] Phase 2 — + gatherer-iron (graph fan-out edge confirmed)
- [x] Phase 3 — coord briefs gatherer-iron (router delivered end-to-end; real CLI trust prompt absorbs the message — needs bash mode for clean signal)
- [x] Phase 4 — gatherer-iron Done (worker→coord direction confirmed via `./demos/factory-line/drive-bash.sh`)
- [x] Phase 5 — + gatherer-coal (registered + parallel-Done verified)
- [x] Phase 6 — + smelter (gated on iron + coal inventories)
- [x] Phase 7 — + stamper (gated on smelter ingot)
- [x] Phase 8 — + assembler (gated on stamper widget)
- [x] Phase 9 — full goal run via `drive-bash.sh`: 5 events, all `delivered=True`, `output/widget.txt` produced with correct contents

## Phase 9 result (bash mode)

End-to-end run via `./demos/factory-line/drive-bash.sh` (bash mode):

```
  gatherer-coal  →  coordinator   Done: gathered 4 coal
  gatherer-iron  →  coordinator   Done: gathered 4 iron-ore
        smelter  →  coordinator   Done: smelted 4 ingots
        stamper  →  coordinator   Done: stamped 4 widgets
      assembler  →  coordinator   Done: assembled 2 gadgets + 2 gizmos
```

Output (`~/github/softwarewrighter/factory-line/output/widget.txt`):

```
gadgets: [g1] [g2]
gizmos: [z1] [z2]
```

Inventory snapshot (each worker maintains its own JSON):

| Agent | items |
|-------|-------|
| gatherer-iron | iron-ore: 4 |
| gatherer-coal | coal: 4 |
| smelter       | ingot: 4 |
| stamper       | widget: 4 |

All 5 events `delivered: True` — router round-trip is fully working
in both directions. The bash drive script is the wiring smoke test;
swapping each agent to its real CLI (claude/codex/opencode) requires
solving the startup-prompt + update-check issues separately.

## Open questions raised by the walkthrough

These accumulate as phases run; they're candidates for follow-up
sagas / bug fixes, not blockers for the walkthrough itself:

- Treemap mode is the forced default (auto-enabled on first agent).
  No UI affordance to opt into the equal-tile CSS grid (`.dashboard.cols-N`).
  Phase 2+ will exercise this — if treemap heat-sizing collapses 5 of 6
  workers to compact tiles even with no activity, that's a usability bug.
- Graph view's `coordinator → worker` edges are static (fan-out
  in `static/index.html:4431-4438`). `feature_request` events do not
  add edges. To make the graph reflect real activity, the renderer
  would need to draw edges from event source→target with a TTL.
- `setFocus` calls `fitAddon.fit()` on every focus change → positive
  feedback loop shrinks the xterm by a column or two each click.
  Documented in `static/index.html` near line 1227.
- Wiki state survives `pkill -9 atn-server`. Tests must explicitly
  wipe `.atn/wiki/` for true empty wiki baselines.
- Real agent CLIs (`claude`, `codex`, `opencode`) start with
  interactive prompts (auth, project-trust, license) that gate input.
  When the router types `[ATN task from coordinator] ...` into a
  worker's PTY before that prompt is cleared, the trust dialog
  consumes the bytes (codex's "1. Yes, continue / 2. No, quit"
  observed in Phase 3). Either:
  - Run the walkthrough with `agent: bash` for everyone (use
    `ATN_DEMO_AGENT=bash ./demos/factory-line/setup.sh`) so input
    arrives at a plain shell — clean signal but no real CLI demo.
  - Add a startup-prompt-clearer (`echo '1' | <cli>`-style or a
    spawned `expect` script) — out-of-scope for v1 of this doc.
- Router timing is `2 s` outbox poll + transcript flush latency.
  Phase tests should `sleep 3` after a POST before checking
  delivery state to avoid false negatives.
