## Step 4: Three-Agent Demo

Deliver fixtures, a demo script, and an integration test that stand up the
full three-agent topology from an empty ATN start by POSTing to `/api/agents`
three times.

### Topology

| Agent          | Transport | User    | Host        | Dir                      | Agent CLI              |
|----------------|-----------|---------|-------------|--------------------------|------------------------|
| coordinator    | local     | mike    | mighty-mike | ~/work/atn-demo          | claude                 |
| worker-hlasm   | mosh      | devh1   | queenbee    | /home/devh1/work/hlasm   | codex                  |
| worker-rpg     | mosh      | devr1   | queenbee    | /home/devr1/work/rpg-ii  | opencode-z-ai-glm-5    |

### Deliverables

1. `demos/three-agent/setup.sh` — boots `atn-server`, waits for /healthz, then
   POSTs the three agent definitions above in order (coordinator first).
2. `demos/three-agent/fixtures/*.json` — one JSON payload per agent matching
   the `POST /api/agents` schema from step 2.
3. Integration test `crates/atn-server/tests/three_agent_demo.rs` that uses
   fake-agent binaries (echo loops) in place of claude/codex/opencode, starts
   from an empty `agents.toml`, posts the three fixtures, and asserts:
   - all three agents reach `running` state
   - each agent appears in the events view and wiki participant list
   - a message posted by coordinator reaches both workers' inboxes
4. Fake agent shims `tools/fake-claude`, `tools/fake-codex`,
   `tools/fake-opencode-glm5` — tiny scripts that identify themselves on
   startup and echo stdin to stdout with a prefix.
5. Instructions in the demo script for swapping in the real CLIs by changing
   a single `ATN_DEMO_REAL=1` env var.

### Acceptance

- Running `demos/three-agent/setup.sh` against an empty-start server produces
  three agents visible in the dashboard with the correct composed commands.
- The integration test passes in CI using fake agents.
- Doc in `docs/demo-three-agent.md` shows both the CI (fake) and live (real)
  paths.
- `cargo test --workspace` green; clippy clean.
