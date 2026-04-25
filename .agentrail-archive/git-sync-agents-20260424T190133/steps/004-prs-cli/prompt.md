## Step 4: atn-cli prs subcommand

### Deliverables

1. New `atn-cli prs` subcommand group:
   - `prs list [--format json|table] [--status open|merged|rejected]`
     — GET /api/prs, table cols: ID / AGENT / BRANCH → TARGET /
     STATUS / SUMMARY (truncated to 80 chars).
   - `prs show <id> [--format json|table]` — GET /api/prs/{id},
     pretty-print all fields; 404 → exit 2.
   - `prs merge <id>` — POST /api/prs/{id}/merge. 200 → exit 0;
     409 → print stderr from the body + exit 2 (matches the
     atn-cli wiki ETag mismatch convention).
   - `prs reject <id>` — POST /api/prs/{id}/reject. 200 → exit 0.
2. Reuse the existing `OutputFormat`, `report_*` helpers; no new
   shared plumbing.
3. Unit tests for `prs` table formatter, status filter, and the
   conflict-error handling stub (mock body parses cleanly).
4. Add a `prs` arc to the existing
   `crates/atn-cli/tests/integration.rs` end-to-end test:
   POST a PR JSON via REST, run `atn-cli prs list` → assert
   id appears, `atn-cli prs merge <id>` → exit 0 + GET /api/prs
   shows status=Merged.

### Acceptance

- `atn-cli prs list` against a server with one open PR prints a
  readable table.
- `atn-cli prs merge <id>` round-trips and the central repo's
  log shows the new commit.
- cargo test + clippy + doc clean.