## Step 5: Docs Refresh

Bring the docs in sync with empty-start + structured new-agent dialog +
three-agent demo.

### Deliverables

1. `docs/usage.md` — update the "starting ATN" section to describe empty start
   and the New Agent dialog. Remove stale references to seed agents.
2. `docs/demo-three-agent.md` — step-by-step walkthrough of the three-agent
   demo from step 4. Cover both CI (fake agents) and live (real claude/codex/
   opencode on queenbee) paths. Screenshots or ASCII sketches of the UI at
   each stage.
3. `docs/uber-use-case.md` — add cross-links to usage.md and
   demo-three-agent.md. Update the "Implementation Notes" section to reflect
   what now actually exists.
4. `docs/status.md` — note that phases 0–8 shipped and the remote-agent demo
   saga completed.
5. `README.md` (if present) — quickstart updated to the empty-start flow.

### Acceptance

- `docs/demo-three-agent.md` is followable by a fresh reader end-to-end.
- All cross-links resolve.
- `cargo doc --workspace --no-deps` builds without warnings.
