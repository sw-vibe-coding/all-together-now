## Step 2: detect → push → record

Replace step 1's stub handler with the real "open a PR" flow.

### Deliverables

1. Marker parser: when the file exists, read its content as
   `key=value` lines (one per line, `#`-comments tolerated):
   - `branch=<name>` (default: current `git rev-parse
     --abbrev-ref HEAD` in `--repo`)
   - `target=<name>` (default `main`)
   - `summary=<text>` (default: `<branch>` ready for review)
   Empty marker is fine — defaults fill in.
2. `git push <remote> <branch>:refs/heads/pr/<agent-id>-<branch>` —
   shells out via `Command::new("git")`, captures stderr into
   the JSON record on failure. Push errors don't move the marker
   (so the next poll retries); they DO log to stderr.
3. Resolve the pushed commit SHA via `git rev-parse <branch>`
   in the repo. Build a `PrRecord` and write it to
   `<prs-dir>/<id>.json` (id = `<agent-id>-<branch>-<short-sha>`).
4. On successful push + write, rename `<repo>/<marker>` →
   `<repo>/<marker>.queued.<short-sha>` so the next poll doesn't
   re-process. Idempotent — if the renamed file already exists,
   bail with a warning.
5. Tests build a tempdir layout: `central.git` (bare), `worktree`
   with a real commit + branch, `prs-dir` empty. Drop the marker,
   call the handler, assert (a) `git ls-remote central` shows
   `refs/heads/pr/<id>`, (b) the JSON record on disk parses back
   into the expected `PrRecord`, (c) the marker is renamed.
   Use `git2`-free pure CLI shell-outs (Cargo: rely on `git` binary
   on PATH, like the integration tests already do).

### Acceptance

- The handler runs against a local bare remote and pushes the
  named branch as `pr/<agent-id>-<branch>`.
- The PR JSON record matches the on-disk shape the server reads
  in step 3.
- cargo test + clippy + doc clean.