# All Together Now — Architecture

## System Overview

```
┌─────────────────────────────────────────────────────────┐
│                     Web UI (Yew/WASM)                   │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐  │
│  │ Agent A  │ │ Agent B  │ │ Agent C  │ │   Wiki    │  │
│  │  Panel   │ │  Panel   │ │  Panel   │ │  Browser  │  │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └─────┬─────┘  │
│       │             │            │              │        │
│       └─────────────┴────────────┴──────────────┘        │
│                          │  SSE ↓  HTTP ↑                │
└──────────────────────────┼───────────────────────────────┘
                           │
┌──────────────────────────┼───────────────────────────────┐
│                    PGM Server (Axum)                      │
│                          │                                │
│  ┌───────────────────────┼────────────────────────────┐  │
│  │              Orchestration Core                     │  │
│  │                                                     │  │
│  │  ┌─────────────┐  ┌──────────┐  ┌──────────────┐  │  │
│  │  │   Session    │  │ Message  │  │   Policy     │  │  │
│  │  │   Manager    │  │ Router   │  │   Engine     │  │  │
│  │  └──────┬──────┘  └────┬─────┘  └──────┬───────┘  │  │
│  │         │              │               │           │  │
│  │  ┌──────┴──────┐  ┌────┴─────┐  ┌─────┴────────┐  │  │
│  │  │   Output    │  │  Wiki    │  │  Agentrail   │  │  │
│  │  │   Parser    │  │  Store   │  │  Integration │  │  │
│  │  └──────┬──────┘  └──────────┘  └──────────────┘  │  │
│  └─────────┼──────────────────────────────────────────┘  │
│            │                                              │
│  ┌─────────┼──────────────────────────────────────────┐  │
│  │    PTY Layer (portable-pty)                         │  │
│  │         │                                           │  │
│  │  ┌──────┴──────┐  ┌────────────┐  ┌─────────────┐  │  │
│  │  │  PTY Agent  │  │ PTY Agent  │  │  PTY Agent  │  │  │
│  │  │  (shell A)  │  │ (shell B)  │  │  (shell C)  │  │  │
│  │  └─────────────┘  └────────────┘  └─────────────┘  │  │
│  └─────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────┘
```

## Two Planes

### Terminal Plane
Raw PTY byte streams — agent I/O. Human keystrokes and coordinator commands
are serialized through a per-agent writer queue and written to the PTY master.
Output is read continuously and forwarded to the UI via SSE.

### Orchestration Plane
Structured JSON events between PGM subsystems, the wiki store, and the web UI.
Routing decisions, state transitions, and workflow tracking happen here — never
by scraping terminal output when avoidable.

## Crate Architecture

```
Cargo workspace
├── crates/
│   ├── atn-core/           Domain types, traits, error types
│   │   ├── agent.rs        AgentConfig, AgentState, AgentStatus
│   │   ├── event.rs        PushEvent, InputEvent, OutputEvent
│   │   ├── router.rs       MessageRouter trait
│   │   └── error.rs        Error enum
│   │
│   ├── atn-pty/            PTY session management
│   │   ├── session.rs      PtySession: spawn, read, write, send_ctrl_c
│   │   ├── manager.rs      SessionManager: lifecycle for N agents
│   │   ├── parser.rs       Output parser: prompt detection, idle, question
│   │   └── writer.rs       Serialized writer queue per agent
│   │
│   ├── atn-server/         Axum HTTP server
│   │   ├── main.rs         Server entry point
│   │   ├── routes.rs       REST + SSE endpoints
│   │   ├── sse.rs          SSE streaming per agent
│   │   └── state.rs        Shared application state (Arc)
│   │
│   ├── atn-ui/             Yew web UI components
│   │   ├── app.rs          App shell, routing
│   │   ├── agent_panel.rs  Per-agent terminal view + input
│   │   ├── dashboard.rs    Grid of agent panels + status overview
│   │   ├── wiki_view.rs    Embedded wiki browser (from wiki-rs)
│   │   └── nav.rs          Navigation
│   │
│   ├── atn-wiki/           Wiki integration (adapted from wiki-rs)
│   │   ├── storage.rs      WikiStorage trait (reused from wiki-rs)
│   │   ├── coordination.rs Coordination pages: goals, blockers, requests
│   │   └── renderer.rs     Markdown + wiki-link rendering
│   │
│   └── atn-trail/          Workflow tracking (adapted from agentrail-rs)
│       ├── saga.rs         Per-agent saga lifecycle
│       ├── step.rs         Step state machine
│       ├── trajectory.rs   ICRL trajectory recording
│       └── skill.rs        Skill distillation & injection
│
└── frontend/               Trunk-built WASM binary
    └── index.html
```

## Integration with Existing Projects

### wiki-rs
ATN reuses wiki-rs's core: `WikiPage` model, `WikiStorage`/`AsyncWikiStorage`
traits, markdown rendering with wiki-links, CAS concurrency (ETag-based),
and the PATCH API. The Yew UI components from wiki-rs are adapted into
`atn-ui` for the embedded wiki browser.

**What ATN adds**: Coordination-specific wiki pages (goals, blockers, agent
assignments), push-triggered wiki reads, and routing-aware page updates.

### agentrail-rs
ATN reuses agentrail-rs's core: saga/step state machine, trajectory recording,
skill distillation, and the `next` command's ICRL injection. These are
adapted into `atn-trail` as a library (no CLI dependency).

**What ATN adds**: Per-agent saga instances managed by the PGM, automatic
trajectory recording on step completion, and UI visibility into workflow state.

## Key Design Decisions

### D1: PGM owns PTYs (not terminal tabs)
The PGM spawns agent shells via `portable-pty`, giving it deterministic
control over input/output without OS-level window automation hacks.

### D2: Serialized writer per agent
Each agent PTY has exactly one writer queue. Human input, coordinator commands,
and macro injections are enqueued and written sequentially. No interleaving.

### D3: Library-first with pluggable UI
`atn-core`, `atn-pty`, `atn-wiki`, and `atn-trail` are pure library crates
with no UI dependency. The Yew UI (`atn-ui`) is one frontend. Future
frontends (CLI, TUI via ratatui, Emacs via emacs-module-rs) consume the
same library API.

### D4: Wiki as durable state, PTY as push mechanism
The wiki stores coordination state that persists across agent context windows.
PTY injection is the push/notification layer. Neither replaces the other.

### D5: Structured events over terminal scraping
Inter-agent communication uses typed events (feature_request, completion_notice,
etc.) routed through the orchestration plane. Terminal output parsing is
limited to prompt detection and idle/question heuristics.

## Data Flow

```
Human (browser)
  │
  ├── text input ──→ PGM ──→ agent PTY writer queue ──→ PTY master
  ├── Ctrl-C ──────→ PGM ──→ 0x03 byte to PTY master
  └── wiki edit ───→ PGM ──→ wiki store ──→ optional broadcast nudge

Agent (PTY output)
  │
  ├── terminal bytes ──→ PGM ──→ SSE ──→ browser terminal widget
  ├── structured push ──→ PGM ──→ router ──→ target agent PTY or wiki
  └── step complete ───→ PGM ──→ agentrail trajectory ──→ saga advance

PGM (coordinator)
  │
  ├── detect idle/question ──→ UI badge "awaiting input"
  ├── route request ──────→ target agent PTY or wiki + broadcast
  └── record trajectory ──→ agentrail store
```

## Technology Stack

| Layer | Technology |
|-------|-----------|
| PTY management | `portable-pty` |
| Async runtime | `tokio` |
| HTTP server | `axum` (REST + SSE) |
| Web UI | `yew` (WASM via Trunk) |
| Terminal widget | `xterm.js` (in browser) |
| Wiki storage | Adapted from `wiki-rs` (file, SQLite, or git backend) |
| Workflow tracking | Adapted from `agentrail-rs` (file-based sagas) |
| Serialization | `serde`, `serde_json`, `toml` |
| Logging | `tracing` |
