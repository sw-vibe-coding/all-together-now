## Step 5: atn-cli — integration tests + docs + Demo 10

Harden atn-cli with an end-to-end test that boots a real server and
exercises every subcommand, then document the tool.

### Deliverables

1. `crates/atn-cli/tests/integration.rs`:
   - Spawns a fresh `atn-server` on an ephemeral port (same pattern
     as `screenshot_endpoint.rs`).
   - Uses `Command::new(CARGO_BIN_EXE_atn-cli)` (Cargo's built-in
     bin-under-test discovery) to exercise:
     - `agents list` on an empty server
     - spawn an agent via REST, then `agents state <id>`, then
       `agents wait <id> --state idle --timeout 10`
     - `agents input <id> echo HELLO` + `agents screenshot <id>`
       and assert the screenshot contains `HELLO`
     - `events send --from <a> --kind completion_notice --summary x`
       + `events list` and assert the entry shows up
     - `wiki get Coordination/Goals` returns the seeded body
   - All assertions on captured stdout / exit code; no Playwright
     needed.

2. `docs/atn-cli.md`:
   - Short intro + exit code table + `ATN_URL` / `--base-url`.
   - One section per subcommand group (agents / events / wiki),
     copying the happy-path example from the integration test.
   - "Script recipes": 3-agent seed, wait-for-all-idle,
     tail-events-since.

3. `docs/demos-scripts.md`:
   - New **Demo 10 — atn-cli tour** with setup / steps / cleanup
     that mirrors the integration test.
   - "Picking one for a short slot" section: add "Integrations /
     API-minded audience: Demo 10 (atn-cli) — 5 min".

4. `docs/status.md`: new `C1..C5` rows for the atn-cli saga.

### Acceptance

- `cargo test -p atn-cli` runs the integration test green.
- `cargo test --workspace` stays green; `cargo clippy` + `cargo doc`
  warning-free.
- docs/atn-cli.md readable end-to-end.