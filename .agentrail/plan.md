# Dashboard PRs

Surface the `/api/prs` registry inside the dashboard so humans
review and merge PRs without dropping into `atn-cli`. Builds on
the git-sync-agents saga: closes the loop from agent ➜ syncd ➜
PrRecord ➜ **dashboard** ➜ merge.

## Goals

- Live PR list rendered next to the dashboard, no manual refresh.
- Detail panel with the git diff for the PR commit.
- Merge / reject buttons that round-trip the existing
  `POST /api/prs/{id}/{merge,reject}` routes; conflict stderr
  surfaces in a modal.
- Status / agent filters + free-text search, persisted in
  `localStorage` like the events-view filters.
- Docs + demo update so Demo 14 walks through the UI flow on top
  of the two-agent Demo 13 fixture.

## Acceptance

- `cargo test --workspace` green; `cargo clippy --workspace
  --all-targets -D warnings` clean; `cargo doc --workspace
  --no-deps` warning-free.
- Live demo: drop markers on alice + bob (Demo 13 setup), open
  the dashboard PR panel, see both PRs appear without polling,
  click "Merge" on each, see the central main log gain both
  commits, and watch the PR cards flip to `merged` in-place.
