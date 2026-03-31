# Needed Rust CLI Tools

The demo (`demo/run-demo.sh`) currently uses `curl` + `jq` for REST API interaction
and shell scripting for orchestration. These use-cases would benefit from purpose-built
Rust CLI tools.

## 1. `atn-cli` — Command-line client for atn-server

Replaces: `curl` + `jq` calls to the REST API throughout the demo script.

### Subcommands

```
atn-cli agents                       # List agents with current state
atn-cli agents wait <id> --idle      # Poll until agent reaches idle (replaces wait_for_idle loop)
atn-cli agents input <id> <text>     # Send text input to agent PTY
atn-cli agents stop <id>             # Stop an agent
atn-cli agents restart <id>          # Restart an agent

atn-cli events list [--since N]      # Query event log
atn-cli events send \                # Submit a push event for routing
    --from <agent> \
    --to <agent> \
    --kind <verification_request|completion_notice|...> \
    --summary "message text" \
    [--priority high]

atn-cli wiki get <title>             # Read a wiki page
atn-cli wiki put <title> <content>   # Write a wiki page
```

### Why Rust instead of shell

- **Type-safe event construction**: The PushEvent struct has required fields (id, kind,
  source_agent, source_repo, timestamp) that are error-prone to construct as raw JSON
  in shell. A Rust CLI can generate IDs and timestamps automatically.
- **Reliable state polling**: The `wait_for_idle` shell function is a fragile polling
  loop. A Rust tool can handle AgentState enum variants properly (some are objects like
  `{"blocked":{"on":["alice"]}}`), implement exponential backoff, and provide a clean
  exit code.
- **Structured output**: `--format json` for scripting, human-readable tables by default.

### Shell usage it replaces

```bash
# Before (shell):
curl -sf -X POST "http://localhost:7500/api/agents/dev/input" \
  -H 'Content-Type: application/json' \
  -d '{"text":"echo hello"}'

# After (atn-cli):
atn-cli agents input dev "echo hello"
```

```bash
# Before (shell):
while true; do
  state=$(curl -sf http://localhost:7500/api/agents/dev/state | jq -r '.state')
  [[ "$state" == "idle" ]] && break
  sleep 1
done

# After (atn-cli):
atn-cli agents wait dev --idle --timeout 30
```

---

## 2. `atn-agent` — AI coding agent wrapper

Replaces: `opencode run` + manual outbox/inbox JSON file management.

This is a standalone process suitable for use as `launch_command` in agents.toml.
It wraps an LLM API (ollama or opencode-compatible) with tool-calling, and integrates
with ATN's inbox/outbox messaging.

### Core loop

```
1. Check inbox (.atn/inboxes/<agent-id>/) for new messages
2. If message found → construct prompt with message context
3. Call LLM with tool definitions:
   - file_read(path) → read a file
   - file_write(path, content) → write a file
   - shell_exec(command) → run a shell command
   - outbox_send(target, kind, summary) → write to outbox
   - inbox_ack(message_id) → mark inbox message as handled
4. Execute tool calls, feed results back to LLM
5. Repeat until LLM signals done
6. Return to step 1 (poll inbox)
```

### Why Rust instead of Python

- **No Python dependency**: The project is pure Rust. Adding Python for a single script
  introduces a runtime dependency, virtualenv management, and version concerns.
- **Ollama API is simple HTTP**: The ollama `/api/chat` endpoint with tool-calling is
  just JSON over HTTP — straightforward in Rust with `reqwest`.
- **Type-safe tool definitions**: Tool schemas for ollama's function-calling format can
  be defined as Rust structs with serde, avoiding hand-crafted JSON.
- **Process lifecycle**: As a `launch_command`, this tool runs in a PTY managed by ATN.
  Rust gives clean signal handling and no interpreter startup overhead.

### Configuration

```toml
# In agents.toml:
[[agent]]
id = "dev"
name = "Developer"
launch_command = "atn-agent --model qwen3:8b --provider ollama"
```

Or for remote models:
```toml
launch_command = "atn-agent --model cerebras/llama3.1-8b --provider opencode"
```

---

## 3. `atn-test` — Test harness helper (lower priority)

Replaces: Server lifecycle management in the demo script (start, wait, cleanup).

```
atn-test run demo/run-demo.sh \
    --server-bin target/release/atn-server \
    --config demo/demo-agents.toml \
    --timeout 120
```

This would:
- Create an isolated temp workspace
- Start atn-server with the given config
- Run the test script
- Capture output
- Kill server and clean up on exit

Lower priority because the shell script handles this adequately, and this tool
has a narrower use-case than `atn-cli` or `atn-agent`.

---

## 4. `atn-pty-screenshot` — PTY terminal screenshot capture

Captures a rendered "screenshot" of an agent's terminal state for diagnostics.

### Problem

The SSE endpoint streams raw PTY bytes (escape sequences, cursor moves, etc.).
To get a human-readable snapshot of what the terminal looks like at a given moment,
you need a virtual terminal emulator to process those bytes.

### Implementation

- Use the `vte` crate to process the SSE byte stream through a virtual terminal
- Render the terminal grid (rows x cols) as plain text
- Output as text, ANSI-colored text, or HTML

```
atn-pty-screenshot dev             # text snapshot
atn-pty-screenshot dev --ansi      # with colors
atn-pty-screenshot dev --html      # rendered HTML
```

### Use-cases

- Diagnostic logging in CI/CD and tests
- Debugging agent PTY state issues
- Capturing proof of AI agent output for audits

---

## 5. Agent watchdog / health monitor

### Problem

ATN's state tracker detects idle (5s no output → idle) but has no watchdog for:
- Deadlocks (agent stuck indefinitely in `running`)
- Premature exits (process died but PTY still open)
- Hung AI commands (opencode stalled on API timeout)
- Infinite loops in agent scripts

### Needed capabilities

- Configurable per-agent timeout: max time in `running` state before escalation
- Process liveness check: verify the child process is still alive
- Output stall detection: no PTY output for N seconds while state is `running`
- Recovery actions: send Ctrl-C, restart agent, escalate to human
- Integration with event system: post `blocked_notice` events automatically
