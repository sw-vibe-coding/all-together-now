## Step 1: Empty Start

ATN must boot cleanly with zero configured agents. The UI must render a deliberate
empty state whose only affordance is a "New Agent" call-to-action.

### Deliverables

1. `agents.toml` is allowed to contain zero `[[agent]]` entries; server does not
   panic, crash, or fall back to seeded demo agents.
2. Remove the seed `alice`/`bob`/`carol` agents from the repo's `agents.toml`.
   Preserve them as `agents.example.toml` or similar for reference.
3. UI dashboard, events view, wiki participant list, and every other per-agent
   surface render a non-broken empty state when the agent list is empty.
4. The empty state surfaces a prominent "New Agent" button that will drive the
   dialog added in step 2. For now it can be a stub that logs/toasts.
5. Existing tests still pass after the seed removal; add a new test that spins
   up the server with an empty agents.toml and asserts healthy boot.

### Acceptance

- `cargo run -p atn-server` with empty `agents.toml` boots; `/api/agents`
  returns `[]`.
- Dashboard at `/` loads without errors or blank panels.
- `cargo test --workspace` green.
- `cargo clippy --workspace -- -D warnings` clean.
