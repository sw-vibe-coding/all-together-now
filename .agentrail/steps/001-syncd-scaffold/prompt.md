## Step 1: atn-syncd scaffold + marker detection

Stand up the `atn-syncd` crate. Clap CLI + watcher loop that polls
a repo path for a marker file. No git actions yet — this step is
lifecycle + filesystem watching only.

### Deliverables

1. New `crates/atn-syncd/` workspace member with
   `[[bin]] name = "atn-syncd"`. Deps: clap derive, serde +
   serde_json, atn-core (PrRecord type lands here in step 2 — for
   step 1 just the crate boilerplate).
2. Add a new `atn-core::pr` module with a placeholder
   `PrRecord { id, agent_id, source_repo, branch, target, commit,
   summary, status, created_at }`. `status` is an enum
   `Open | Merged | Rejected`. Unit tests for serde round-trip.
3. CLI flags:
   - `--repo <PATH>` (required) — agent's git worktree.
   - `--agent-id <ID>` (required) — used to namespace the pushed branch.
   - `--remote <NAME>` (default `central`) — git remote name.
   - `--marker <FILE>` (default `.atn-ready-to-pr`) — relative to the
     repo root.
   - `--prs-dir <PATH>` (default `.atn/prs`) — where PR records land.
   - `--poll-secs <N>` (default 3).
   - `--dry-run` — log instead of push / write.
   - `--exit-on-empty` — exit cleanly after one marker-free pass
     (for tests).
   - `--verbose` — log every poll tick.
4. Main loop:
   - Print banner `atn-syncd: <agent-id> watching <repo>`.
   - Every `--poll-secs`, check `<repo>/<marker>` existence.
   - Log presence/absence; the action handler is a stub that just
     prints `would handle <marker>` for now.
5. 3–4 unit tests (resolve_marker_path, args defaults,
   serde round-trip for PrRecord, status enum serialization).

### Acceptance

- `cargo run -p atn-syncd -- --repo /tmp/x --agent-id alice
   --dry-run --exit-on-empty` prints the banner + exits cleanly.
- `cargo test --workspace` green; clippy + doc warning-free.