# All Together Now — Design

## Core Abstractions

### Agent Session

An `AgentSession` represents one managed AI agent running in a PTY.

```rust
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub repo_path: PathBuf,
    pub role: AgentRole,           // Developer, QA, PM, Coordinator
    pub setup_commands: Vec<String>, // env switch, cd, etc.
    pub launch_command: String,    // e.g., "claude --dangerously-skip-permissions"
}

pub enum AgentState {
    Starting,
    Running,
    AwaitingHumanInput,
    Busy,
    Blocked { on: Vec<String> },
    Idle,
    CompletedTask,
    Error(String),
    Disconnected,
}

pub struct AgentStatus {
    pub config: AgentConfig,
    pub state: AgentState,
    pub last_output_at: Instant,
    pub pending_requests: Vec<PushEvent>,
    pub current_task: Option<String>,
    pub saga_step: Option<(u32, String)>, // (step_number, slug)
}
```

### Input Model

All input to an agent PTY flows through a single serialized queue:

```rust
pub enum InputEvent {
    /// Human-typed text (written as-is + newline)
    HumanText(String),
    /// Raw bytes (Ctrl-C = 0x03, arrow keys, etc.)
    RawBytes(Vec<u8>),
    /// Coordinator-generated command (written as-is + newline)
    CoordinatorCommand(String),
    /// Canned action
    Action(CannedAction),
}

pub enum CannedAction {
    CtrlC,
    ClaudeGo,          // Ctrl-C wait, then "claude go\n"
    ReadWiki(String),   // "coord inbox\n" or specific page
    Ack(String),        // "coord ack REQ-{id}\n"
}
```

### Output Parsing

The output parser monitors PTY output and emits structured events:

```rust
pub enum OutputSignal {
    /// Raw bytes for terminal rendering
    Bytes(Vec<u8>),
    /// Detected that agent is at a prompt (ready for input)
    PromptReady,
    /// Agent appears to be asking a question (numbered options, "?")
    QuestionDetected { snippet: String },
    /// No output for N seconds while at prompt
    IdleDetected,
    /// Agent emitted a structured push event (JSON on a known channel)
    PushEvent(PushEvent),
}
```

Prompt detection uses a configurable prompt pattern. For Claude Code, this
likely involves recognizing the tool's prompt after output settles. The MVP
uses an idle timer + heuristic pattern matching; later versions can use a
custom sentinel if we control the shell prompt wrapping Claude.

### Push Events (Inter-Agent)

```rust
pub struct PushEvent {
    pub id: String,
    pub kind: PushKind,
    pub source_agent: String,
    pub source_repo: String,
    pub target_agent: Option<String>,
    pub issue_id: Option<String>,
    pub summary: String,
    pub wiki_link: Option<String>,
    pub priority: Priority,
    pub timestamp: String,
}

pub enum PushKind {
    FeatureRequest,
    BugFixRequest,
    CompletionNotice,
    BlockedNotice,
    NeedsInfo,
    VerificationRequest,
}

pub enum Priority {
    Normal,
    High,
    Blocking,
}
```

### Message Router

```rust
pub trait MessageRouter: Send + Sync {
    /// Route a push event to the appropriate destination(s).
    fn route(&self, event: PushEvent) -> Vec<RouteAction>;
}

pub enum RouteAction {
    /// Inject command into target agent's PTY
    InjectToAgent { agent_id: String, command: String },
    /// Update wiki page
    UpdateWiki { page: String, content: String },
    /// Broadcast "read coordination page" to all agents
    BroadcastWikiRead { page: String },
    /// Surface in human UI for manual routing
    EscalateToHuman(PushEvent),
}
```

The default router:
- **Target known** → inject into target agent's PTY (wait for prompt-ready).
- **Target unknown** → update wiki coordination page + broadcast nudge.
- **Human review required** → escalate to UI.

## Wiki Integration

### Coordination Pages

ATN maintains well-known wiki pages for coordination:

| Page | Purpose |
|------|---------|
| `Coordination/Goals` | Overall project objectives |
| `Coordination/Agents` | Who is working on what |
| `Coordination/Requests` | Open requests (feature, bug fix) |
| `Coordination/Blockers` | Current blockers and dependency chain |
| `Coordination/Log` | Append-only event log |

### CAS for Multi-Agent Writes

When multiple agents or the PGM update wiki pages, CAS (Compare-and-Swap)
via ETags prevents lost updates. On conflict, the PGM retries with a merge
strategy (append for log pages, replace for status pages).

### Wiki Storage Backend

For MVP, use the file-based backend from wiki-rs (each page = a `.md` file).
The SQLite or git backends can be swapped in later without changing the
`AsyncWikiStorage` trait boundary.

## PTY Management

### Spawn Sequence

```
1. Create PTY pair (master/slave) via portable-pty
2. Spawn shell into slave
3. Inject setup commands:
   a. export PS1="__ATN_READY__> "
   b. cd /path/to/repo
   c. source .coord/agent-shell.sh  (optional helper)
4. Wait for __ATN_READY__ prompt
5. Launch Claude Code: claude [args]
6. Mark agent state: Running
7. Start output reader task
8. Start writer queue consumer task
```

### Safe Injection Protocol

Before injecting a command:
1. Check agent state is `PromptReady` or `Idle`.
2. If agent is `Busy`, enqueue as pending.
3. If agent is `AwaitingHumanInput`, only human input is accepted.
4. Write command bytes + newline to PTY master.
5. Set state to `Busy`.

For Ctrl-C:
1. Write `0x03` to PTY master.
2. Wait for output to settle (configurable timeout, default 2s).
3. Check for prompt.
4. If sending `claude go` after, wait for prompt-ready before writing.

### Shutdown Sequence

1. Send Ctrl-C (0x03) twice with 1s delay (Claude Code needs 2 to quit).
2. Wait for child process exit (timeout 5s).
3. If still alive, send SIGTERM.
4. Close PTY master.

## Agentrail Integration

Each agent can optionally have an associated agentrail saga:

- **Saga init**: PGM creates a saga in `.agentrail/` within the agent's repo
  when starting a multi-step workflow.
- **Step tracking**: When the PGM detects a step completion (via agent push
  or human marking), it calls agentrail's `complete` logic to record the
  trajectory.
- **Context injection**: On fresh context (new Claude session), the PGM
  can inject `agentrail next` output to provide skill docs and past successes.
- **Distillation**: Periodic skill distillation from accumulated trajectories.

ATN uses agentrail-rs as a library dependency, not as a CLI subprocess.

## Web UI (Yew)

### Page Structure

```
┌─────────────────────────────────────────────────┐
│  Nav: Dashboard | Wiki | Dependency Graph | Log  │
├─────────────────────────────────────────────────┤
│                                                  │
│  Dashboard view:                                 │
│  ┌──────────────┐ ┌──────────────┐              │
│  │ Agent A  [!] │ │ Agent B      │              │
│  │ ┌──────────┐ │ │ ┌──────────┐ │              │
│  │ │ terminal │ │ │ │ terminal │ │              │
│  │ │ (xterm)  │ │ │ │ (xterm)  │ │              │
│  │ └──────────┘ │ │ └──────────┘ │              │
│  │ [input___]   │ │ [input___]   │              │
│  │ [Send][^C]   │ │ [Send][^C]   │              │
│  │ [claude go]  │ │ [claude go]  │              │
│  └──────────────┘ └──────────────┘              │
│                                                  │
│  ┌──────────────┐ ┌──────────────┐              │
│  │ Agent C      │ │ Agent D      │              │
│  │ ...          │ │ ...          │              │
│  └──────────────┘ └──────────────┘              │
│                                                  │
└─────────────────────────────────────────────────┘
```

### Agent Panel Component

Each panel contains:
- **Header**: Agent name, repo, role, state badge (color-coded).
- **Terminal**: `xterm.js` widget receiving raw bytes via SSE.
- **Input**: Text box + Send button. Enter key sends.
- **Actions**: Ctrl-C, `claude go`, Read Wiki, Mark Done.
- **Attention indicator**: `[!]` badge when `AwaitingHumanInput`.

### SSE Streams

One SSE endpoint per agent: `GET /api/agents/{id}/stream`

Event types:
- `terminal` — raw terminal bytes (base64 encoded).
- `state` — agent state change.
- `attention` — agent needs human input.
- `push` — inter-agent push event routed through this agent.

### REST Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/agents` | List all agents with status |
| GET | `/api/agents/{id}` | Agent detail |
| POST | `/api/agents/{id}/input` | Send text input |
| POST | `/api/agents/{id}/ctrl-c` | Send Ctrl-C |
| POST | `/api/agents/{id}/action` | Send canned action |
| GET | `/api/agents/{id}/stream` | SSE terminal stream |
| GET | `/api/wiki/pages` | List wiki pages |
| GET | `/api/wiki/pages/{title}` | Get wiki page |
| PUT | `/api/wiki/pages/{title}` | Save wiki page (CAS) |
| PATCH | `/api/wiki/pages/{title}` | Patch wiki page |
| GET | `/api/events` | Global event log (SSE) |

## Library Boundary

The library crates (`atn-core`, `atn-pty`, `atn-wiki`, `atn-trail`) expose
a `PgmController` that any frontend can drive:

```rust
pub struct PgmController { /* ... */ }

impl PgmController {
    pub async fn new(config: PgmConfig) -> Result<Self>;
    pub async fn spawn_agent(&self, config: AgentConfig) -> Result<AgentId>;
    pub async fn send_input(&self, agent: &AgentId, input: InputEvent) -> Result<()>;
    pub async fn subscribe_output(&self, agent: &AgentId) -> broadcast::Receiver<OutputSignal>;
    pub async fn agent_status(&self, agent: &AgentId) -> Result<AgentStatus>;
    pub async fn list_agents(&self) -> Vec<AgentStatus>;
    pub async fn route_push(&self, event: PushEvent) -> Result<Vec<RouteAction>>;
    pub async fn shutdown_agent(&self, agent: &AgentId) -> Result<()>;
    pub async fn shutdown_all(&self) -> Result<()>;
}
```

A CLI frontend would call `PgmController` directly. A TUI frontend would
wrap it with ratatui. The Yew frontend talks to it via the Axum REST/SSE
layer.
