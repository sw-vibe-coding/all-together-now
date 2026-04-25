ATN — atn-agent

Rust-native AI-coding-agent wrapper. Stands in as an agent
`launch_command` the same way `claude` / `codex` / `opencode-z-ai-glm-5`
do today. Wraps an Ollama-compatible HTTP endpoint with tool-calling
and integrates with ATN's inbox/outbox file-based messaging —
replaces the `opencode run` + hand-written JSON scaffolding in
`docs/needed-tools.md §2`.

## Why

- Every demo outside `fake-*` shims needs a real CLI installed
  (`claude` / `codex` / `opencode`). Those are fine for live use
  but heavy for CI + onboarding.
- Ollama is a simple HTTP API; wrapping it in Rust gives us a lean
  agent binary that needs no Python, no node_modules, no global
  install step — just `cargo build -p atn-agent`.
- The tool-calling loop is a well-understood primitive. Getting our
  own version means we control the inbox/outbox plumbing, respect
  `atn-core::inbox` conventions, and print a clean PTY banner the
  dashboard can track.

## Non-goals (this saga)

- Provider-agnostic SDK. We target the Ollama /api/chat shape
  (which opencode mirrors); other providers can land later.
- Agent memory across restarts beyond re-reading the inbox.
- Multi-turn planning. Tool-call loop caps at N iterations; anything
  more elaborate is the next saga's job.

## Steps

1. agent-scaffold — new `atn-agent` crate with a clap CLI
   (`--model`, `--base-url`, `--agent-id`, `--atn-dir`, `--workspace`,
   `--inbox-poll-secs`, `--max-tool-iterations`, `--allow-shell`).
   Main loop: print a banner to stdout so ATN's PTY state tracker
   sees the agent come up, then poll
   `<atn-dir>/inboxes/<agent-id>/` for `.json` messages, ack each
   by renaming to `.json.done`. No LLM calls yet — this step is
   pure lifecycle + inbox plumbing. SIGINT should exit cleanly so
   ATN's `Restart` / shutdown stays responsive.

2. agent-ollama-chat — send a prompt to
   `POST <base-url>/api/chat` shaped as the Ollama chat payload
   `{model, messages:[{role,content}], stream:false, tools:[]}`.
   Build the initial user message from an inbox entry's `summary`
   + `source_agent` + optional `wiki_link` context. Print the
   model's `message.content` to stdout so the dashboard shows
   live activity. Handle transport errors gracefully (log to
   stderr, skip the message, keep polling).

3. agent-tools-fs — implement `file_read(path)` and
   `file_write(path, content)` tools backed by the agent's
   `--workspace` directory. Path validation rejects absolute paths
   and `..` traversal outside the workspace. Tool-call loop:
   dispatch each `message.tool_calls` entry, append a
   `{role: 'tool', content: …}` message with the result, re-POST,
   stop when the model returns no tool_calls or we hit
   `--max-tool-iterations`.

4. agent-tools-shell-outbox — `shell_exec(command)` behind
   `--allow-shell` (default off), runs in the workspace, captures
   combined stdout+stderr, truncates to 4 KiB, times out at 30 s.
   `outbox_send(target, kind, summary)` writes a `PushEvent` JSON
   to `<atn-dir>/outboxes/<agent-id>/`. `inbox_ack(message_id)`
   renames the matching inbox `.json` to `.json.done` (the main
   loop does this too after a successful run, but exposing the
   tool lets the model explicitly ack mid-run).

5. agent-integration-demo-docs — integration test against a stub
   Ollama HTTP server (`TcpListener` + handcrafted JSON responses;
   no external Ollama install). `docs/atn-agent.md` reference.
   Demo 12 "atn-agent end-to-end" in `demos-scripts.md`. New
   `demos/atn-agent/setup.sh` that boots atn-server, starts a
   stub Ollama on a loopback port, creates an agent whose
   launch_command runs `atn-agent --base-url <stub>`, sends an
   inbox message via `atn-cli events send`, and asserts the
   outbox grows. `docs/status.md`: A1..A5 rows.

## Success metrics

- `atn-agent --help` prints sane usage + exit codes.
- End-to-end integration test (stub Ollama + stub inbox) passes.
- `atn-agent --dry-run` exists for smoke-testing the tool loop
  without any LLM call.
- cargo test + clippy + doc clean workspace-wide.
