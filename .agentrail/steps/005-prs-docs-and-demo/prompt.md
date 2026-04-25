## Step 5: docs + Demo 14 + status updates

Close the saga. Capture the new UI in screenshots / commands so
demos and onboarding catch up.

### Deliverables

1. New `docs/dashboard-prs.md`:
   - Architecture sketch — where the PR panel sits in the
     dashboard, the SSE flow from `notify` watcher to
     `EventSource`, and the merge/reject round-trip.
   - Filter UX (chips / dropdown / search / persistence).
   - Conflict modal — where the stderr lives, how to recover.
   - Cross-link to `git-sync-agents.md` (this saga is the UI
     side of that one).
2. New `demos/dashboard-prs/setup.sh`:
   - Reuses the bare-central + alice + bob + atn-server +
     atn-syncd fixture from `demos/git-sync/setup.sh` (factor
     the common bits into a tiny shell library at
     `demos/_lib/git-sync-fixture.sh` if it makes sense; if the
     copy-paste is small, just inline). Drops markers but does
     NOT run `atn-cli prs merge`. Prints
     `open http://127.0.0.1:<port>` and waits for SIGINT, so
     the operator can drive the PR panel by hand.
3. `docs/demos-scripts.md` Demo 14 — UI walkthrough with the
   key clicks: 📋 → see two cards → click PR-A → Diff ▾ →
   Merge → card flips to merged → click PR-B → Reject → card
   flips. Plus the conflict variation. Index row + short-slot
   pick (under "review/coordination flow") + see-also link.
4. `docs/status.md`:
   - Current State → Dashboard-PRs Saga Complete (replace
     git-sync-agents).
   - Promote git-sync-agents to Prior Milestone.
   - DP1..DP5 phase rows.
   - No new crate; what-exists table unchanged.
5. Top-of-`README.md` (or wherever the feature pitch lives) —
   one-line addition under the existing "PR registry" line:
   "+ dashboard panel for live review + merge".

### Acceptance

- `cargo test --workspace` green.
- `clippy --workspace --all-targets -D warnings` clean.
- `cargo doc --workspace --no-deps` warning-free.
- `./demos/dashboard-prs/setup.sh` boots, both markers get
  pushed, and the operator can complete the merge flow in the
  browser without any extra setup.
