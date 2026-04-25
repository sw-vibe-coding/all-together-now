## Step 5: end-to-end demo + docs + status

Close the saga with the full flow: two local "dev user" repos +
a central bare remote + atn-syncd + atn-server + atn-cli.

### Deliverables

1. `crates/atn-syncd/tests/integration.rs` (or extend the existing
   test): builds three tempdirs — `central.git` (bare), `alice`
   worktree, and `prs-dir` — runs `atn-syncd` once via
   `Command::new(env!("CARGO_BIN_EXE_atn-syncd"))` with
   `--exit-on-empty`, drops a marker between runs, asserts the
   bare remote received the branch + the PR JSON exists.
2. `docs/git-sync-agents.md`:
   - Architecture sketch (one-host subset of uber-use-case.md).
   - Marker file format.
   - Daemon CLI reference + exit codes.
   - REST surface + atn-cli subcommands.
   - Known limitations (no GitHub PRs, no diff in dashboard,
     manual mirror-back).
3. `demos/git-sync/setup.sh`:
   - mktemp -d → bare central + two worktrees + prs-dir.
   - Spawns atn-server + two atn-syncd processes (one per
     worktree).
   - Drops markers on both worktrees, waits for the PR JSONs to
     appear, runs `atn-cli prs list` → `atn-cli prs merge` → prints
     the central log to confirm.
   - Cleans up at the end.
4. `docs/demos-scripts.md` Demo 13 + Picking-one-for-a-short-slot
   row + See-also addition (`docs/git-sync-agents.md`).
5. `docs/status.md`: Current State → Git-Sync-Agents Saga Complete.
   atn-syncd in What Exists + architecture list (10 crates).
   G1..G5 rows.

### Acceptance

- `cargo test --workspace` green.
- `clippy --workspace -D warnings` clean.
- `cargo doc --workspace --no-deps` warning-free.
- Demo 13 setup.sh runs end-to-end without manual intervention.