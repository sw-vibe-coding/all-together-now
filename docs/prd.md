# All Together Now — Product Requirements Document

## Problem

When coordinating multiple AI coding agents across repositories, the human
operator becomes the bottleneck:

1. **No push mechanism** — agents don't poll; the human must manually "poke"
   each one to check the wiki, read a request, or verify a fix.
2. **Tab sprawl** — 8+ terminal tabs, each running Claude Code. The human must
   remember which tab needs attention, mentally track dependency chains, and
   context-switch constantly.
3. **Lost coordination state** — each agent has a partial view. The shared wiki
   helps but agents forget across context windows. Workflow progress (sagas,
   steps, trajectories) is scattered or nonexistent.

Two prior approaches (git-based Kanban board; wiki-only coordination) failed
because they lacked a push plane and centralized routing.

## Solution

**All Together Now (ATN)** is a Program Manager (PGM) that owns agent terminal
sessions and acts as the sole broker for human input, agent output, and
inter-agent requests.

### Core Capabilities

| # | Capability | Description |
|---|-----------|-------------|
| 1 | **PTY ownership** | Spawn and own one pseudo-terminal per agent. Send keystrokes (including Ctrl-C), capture output, detect prompt readiness. |
| 2 | **Human-in-the-loop UI** | Web dashboard (Yew/WASM) with one panel per agent: terminal view, input box, action buttons (Send, Ctrl-C, `claude go`). SSE streaming of agent output. |
| 3 | **Message routing** | Route structured inter-agent requests (feature_request, bug_fix_request, completion_notice, etc.) to the correct target agent's PTY or to the wiki for broadcast. |
| 4 | **Shared coordination state** | Integrated wiki (from wiki-rs) for durable shared context: goals, blockers, dependency map, who-is-doing-what. |
| 5 | **Workflow tracking** | Per-agent saga management (from agentrail-rs) for step sequencing, ICRL trajectory recording, and skill distillation. |
| 6 | **Observability** | Transcript logging, per-agent state machine, structured event log, dependency graph visualization. |

### Non-Goals (MVP)

- Fully autonomous agent-to-agent coordination without human approval.
- Support for non-Claude agents (future).
- Production multi-tenant deployment.

## Users

- **Primary**: A developer running multiple Claude Code instances across repos
  in a coordinated workflow. Experienced with CLI tools and AI agents.

## Key Workflows

### W1: Start a coordinated session
1. User defines agents (name, repo path, role, initial prompt).
2. PGM spawns a PTY per agent, runs env setup, launches Claude Code.
3. Web UI shows all agents with live terminal output.

### W2: Respond to an agent question
1. Agent output triggers "awaiting input" detection.
2. UI badges the panel. User types a response.
3. PGM writes the response into the agent's PTY.

### W3: Interrupt and redirect
1. User clicks Ctrl-C on an agent panel.
2. PGM sends `0x03` to the PTY, waits for prompt.
3. User sends `claude go` or a new instruction.

### W4: Inter-agent request
1. Agent A emits a structured push (e.g., "feature X is done").
2. PGM routes to Agent B's PTY (if target known) or updates wiki and
   broadcasts a "read coordination page" nudge.
3. Target agent reads request and responds.

### W5: Workflow checkpoint
1. Agent completes a step. PGM records the trajectory via agentrail.
2. On fresh context, `agentrail next` injects skill docs and past successes.

## Success Metrics

- Human no longer manually switches between 8 terminal tabs.
- Inter-agent requests are routed in <5 seconds (vs. minutes of manual poking).
- Full transcript capture for every agent session.
- Zero input corruption from concurrent writes (serialized writer).

## Constraints

- macOS and Linux (PTY-based; no Windows initially).
- Rust 2024 edition.
- Yew for primary web UI; library core supports future CLI/TUI/Emacs frontends.
- Must work with Claude Code as-is (terminal app with ANSI escape sequences).
