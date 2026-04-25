## Step 3: /api/prs REST surface

Expose the PR registry through the atn-server so the dashboard +
atn-cli have a uniform read/write API.

### Deliverables

1. atn-server CLI gains `--prs-dir <PATH>` (default `<base-dir>/.atn/prs`).
   `SharedState` carries the path. atn-server creates the dir if missing.
2. `GET /api/prs` returns `Vec<PrRecord>` from the directory's
   `*.json` files (sorted lexically). Skips files that fail to parse
   with a warn-level trace. Status filter via query: `?status=open`.
3. `GET /api/prs/{id}` returns the single record or 404.
4. `POST /api/prs/{id}/merge` — runs
   `git merge --no-ff refs/heads/pr/<branch>` on the CENTRAL repo
   (path comes from a new `--central-repo <PATH>` server flag;
   defaults to the directory containing `--prs-dir`'s parent).
   On success, mutate the JSON to `status: Merged` + write
   `merged_at` + `merge_commit`. Returns 200 + the updated record.
   On merge conflict: leave status `Open`, return 409 with
   `{error, stderr}` body so the user can resolve manually.
5. `POST /api/prs/{id}/reject` — sets `status: Rejected` + writes
   `rejected_at`. Returns 200 + record. No git side effects.
6. Integration test in `crates/atn-server/tests/`:
   - Build a bare `central.git` + worktree + prs-dir layout in
     a tempdir.
   - POST a PR record JSON manually (skip syncd here for isolation).
   - Push a real branch into the bare remote.
   - Boot atn-server with `--prs-dir` + `--central-repo`.
   - Call `GET /api/prs` → see one entry.
   - Call `POST /api/prs/{id}/merge` → confirm the central worktree
     has the new commit on `main`, and the JSON is updated.
   - Call `POST /api/prs/{id}/reject` on a different PR → status
     flips.

### Acceptance

- Round-trip merge lands a commit on the central main branch.
- Bad id → 404. Conflict → 409 with stderr in the body.
- cargo test + clippy + doc clean.