## Step 5: integration test + docs + Demo 12

Close the saga: integration test against a stub Ollama, new docs,
Demo 12, status rows.

### Deliverables

1. `crates/atn-agent/tests/integration.rs`:
   - Spawn a tiny in-process HTTP stub listening on
     `127.0.0.1:0` that answers `POST /api/chat` with canned
     JSON responses:
       (a) first call returns a `file_write` tool_call,
       (b) second call (after tool result) returns a final
           `outbox_send` tool_call,
       (c) third call returns plain content + no tool_calls
           (loop exits).
   - Spawn `atn-agent` via `Command::new(env!("CARGO_BIN_EXE_atn-agent"))`
     pointing at the stub, with a tempdir inbox seeded with one
     message.
   - Assert: the workspace file was created (file_write ran),
     the outbox JSON appeared (outbox_send ran), the inbox
     file was renamed to `.json.done`, and the agent exited 0
     after handling the message (use a one-shot flag, see below).
2. Add a `--exit-on-empty` flag (or reuse `--dry-run` semantics)
   so the integration test can run a single pass without tearing
   down the agent. Document it in the CLI help.
3. `docs/atn-agent.md`:
   - Overview, CLI reference, each tool with examples.
   - Wiring into ATN via `launch_command = "atn-agent --agent-id
     X --base-url http://127.0.0.1:11434 --model qwen3:8b
     --allow-shell"`.
   - Security notes (shell gating, path sandboxing, workspace root).
4. `demos/atn-agent/setup.sh`:
   - Boots `atn-server`, starts the integration-test stub on a
     free loopback port (or calls a helper `tools/atn-agent-stub`
     if we ship one), creates an agent whose `launch_command`
     uses `atn-agent`, sends an inbox message via
     `atn-cli events send`, polls the outbox to confirm delivery,
     prints a guided-tour epilog.
5. `docs/demos-scripts.md` Demo 12 "atn-agent end-to-end".
   Picking-one-for-a-short-slot + See-also additions.
6. `docs/status.md`: A1..A5 rows + Current State flip.

### Acceptance

- `cargo test -p atn-agent --test integration` green.
- `cargo test --workspace` stays green.
- `cargo clippy --workspace -- -D warnings` clean.
- `cargo doc --workspace --no-deps` warning-free.
- docs/atn-agent.md readable end-to-end.