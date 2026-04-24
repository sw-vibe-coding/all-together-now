# atn-agent

`atn-agent` is a Rust-native AI-coding-agent wrapper. It runs as an
ATN `launch_command` on the agent side of a PTY, polls its inbox,
and drives an Ollama-compatible tool-calling LLM to respond.
Replaces `opencode run` + hand-rolled JSON for demos that want a
real agent without the Python/Node stack.

## Quick start

```bash
cargo build -p atn-agent
./target/debug/atn-agent --help
```

An agent spawned by ATN with a launch_command like:

```toml
[[agent]]
id = "demo"
launch_command = "atn-agent --agent-id demo --base-url http://localhost:11434 --model qwen3:8b --allow-shell --atn-dir .atn --workspace ."
```

…prints a `atn-agent: demo up (model=qwen3:8b, base_url=…)` banner
into the PTY (ATN's state tracker flips it to `Running`), polls
`.atn/inboxes/demo/*.json` every `--inbox-poll-secs` (default 2),
and — for each message — calls Ollama's `POST /api/chat` with a
system + user prompt and the 5 registered tool schemas.

## CLI reference

| Flag                    | Default                  | Purpose                                                          |
|-------------------------|--------------------------|------------------------------------------------------------------|
| `--agent-id <ID>`       | *required*               | Matches the ATN agent id; used for inbox + outbox paths.         |
| `--base-url <URL>`      | `http://localhost:11434` | Ollama-compatible host.                                          |
| `--model <NAME>`        | `qwen3:8b`               | Model id passed to the `/api/chat` request.                      |
| `--atn-dir <PATH>`      | `.atn`                   | ATN coordination root; contains `inboxes/` + `outboxes/`.        |
| `--workspace <PATH>`    | `.`                      | Sandbox root for `file_*` + `shell_exec`.                        |
| `--inbox-poll-secs <N>` | `2`                      | Seconds between inbox scans.                                     |
| `--max-tool-iterations` | `8`                      | Cap on tool-call loop iterations per inbox message.              |
| `--allow-shell`         | off                      | Gates `shell_exec`. Off → the tool returns `{disabled: true}`.   |
| `--dry-run`             | off                      | Skip LLM calls; log `would POST /api/chat for <id>`.             |
| `--exit-on-empty`       | off                      | Exit 0 after the first inbox pass that finds nothing. For tests. |
| `--verbose`             | off                      | Log every poll tick + tool call argument payload.                |

### Exit codes

| Code | Meaning                                                        |
|------|----------------------------------------------------------------|
| `0`  | Clean exit (SIGINT from ATN, `--exit-on-empty` reached empty). |
| `1`  | Usage error (invalid agent-id, missing required flags).        |
| `2`  | IO error creating inbox dir / unrecoverable read failure.      |

## Tools

All tools are advertised via `ChatRequest.tools` so the model can
opt in. Each tool's JSON result gets appended to the conversation
as a `{role: "tool", content, tool_call_id}` turn before the next
`/api/chat` call.

### `file_read(path)`

Read a workspace-relative file. Caps at `FILE_READ_MAX = 256 KiB`;
larger files come back with `truncated: true` + `notice`. Absolute
paths and `..` traversal are rejected before the read.

```json
{"path": "notes.md", "bytes": 32, "content": "# Goals from the model\n"}
```

### `file_write(path, content)`

Create or overwrite a workspace file. Creates parent dirs. Refuses
`content > FILE_WRITE_MAX = 1 MiB`. Returns `{path, bytes, message}`.

### `shell_exec(command)`

Runs `/bin/sh -c <command>` with `current_dir = workspace`. Gated:
if the agent was started without `--allow-shell`, the tool returns
`{disabled: true, message: …}` (not an error — the model needs to
know it can stop asking). When enabled:

- 30 s timeout (`SHELL_EXEC_TIMEOUT`); child is killed on deadline.
- Combined stdout + stderr truncated to 4 KiB (`SHELL_OUTPUT_MAX`).
- Returns `{command, exit_code, bytes, output, truncated?, timed_out?}`.

### `outbox_send(target?, kind, summary, priority?)`

Build + write a `PushEvent` JSON to the ATN message router's
inbox, exactly where the browser UI + atn-cli emit events. The
agent's id auto-populates `source_agent`; `kind` + `priority`
validate against the same enums atn-cli enforces (hyphen aliases
ok). Broadcast (no `target`) works. Returns
`{event_id, path, message}`.

### `inbox_ack(message_id)`

Rename `<message_id>.json` → `.json.done` in the agent's inbox.
The main poll loop auto-acks after each chat turn; this tool lets
the model explicitly ack mid-run when a single prompt handles
multiple inbox messages. `message_id` is validated against path
injection (no `/`, `\`, `..`).

## Security notes

- **Path sandboxing.** `sandbox_path` walks `Path::components()`
  and rejects `RootDir`, `Prefix`, and `ParentDir` before joining
  to the workspace. `file_*` and the workspace resolution all
  route through it.
- **Shell gating.** `shell_exec` is off by default. Enable with
  `--allow-shell` only when the workspace is one you're comfortable
  running model-generated commands in.
- **Workspace-relative, not network-sandboxed.** A running shell
  command can still reach the network; use a VM / container if
  that's a concern.
- **Transport timeouts.** `/api/chat` calls time out at 60 s.
  `shell_exec` kills the child at 30 s. Tool-call loops stop at
  `--max-tool-iterations` to keep a stuck model from running away.

## Testing

Two unit modules in `crates/atn-agent/src/{llm,tools}.rs` cover 39
cases. The end-to-end integration test in
`crates/atn-agent/tests/integration.rs` boots a TCP-listener stub
that hand-rolls HTTP/1.1 responses to `/api/chat` — no external
Ollama install required. The stub feeds canned responses in order
(tool_call → tool_call → final content); the test asserts the
workspace file + outbox PushEvent + `.json.done` rename all land.

```bash
cargo test -p atn-agent               # unit + integration
cargo test -p atn-agent --test integration   # just end-to-end
```

See also:

- [docs/demos-scripts.md § Demo 12](./demos-scripts.md#demo-12--atn-agent-end-to-end) — scripted walkthrough.
- [docs/atn-cli.md](./atn-cli.md) — the client the agent talks back through (events + wiki).
- [docs/windowed-ui.md](./windowed-ui.md) — how the agent's PTY appears in the dashboard.
