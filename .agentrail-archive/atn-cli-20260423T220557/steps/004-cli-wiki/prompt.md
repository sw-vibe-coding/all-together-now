## Step 4: atn-cli — wiki list/get/put/delete with ETag

### Deliverables

1. `atn-cli wiki list [--format json|table]` — GET `/api/wiki`.
   Table columns: title, updated_at, version (if present).
2. `atn-cli wiki get <title>` — GET `/api/wiki/{title}`. Prints the
   rendered markdown body to stdout; with `--verbose` prints the
   ETag header to stderr for scripting.
3. `atn-cli wiki put <title> [--file <path> | --stdin]
    [--if-match <etag>]` — reads content from the file or stdin,
   sends PUT `/api/wiki/{title}` with the `If-Match` header when
   `--if-match` is given. On 412 Precondition Failed, print
   "ETag mismatch — refetch and retry" to stderr and exit 2 so
   scripts can loop cleanly.
4. `atn-cli wiki delete <title> [--if-match <etag>]` — DELETE with
   optional `If-Match`. Same 412 handling as put.
5. Unit tests for the body-source selection (file vs stdin) and
   the 412 error path.

### Acceptance

- `atn-cli wiki get Coordination/Goals` prints the seeded page.
- `atn-cli wiki put Coordination/Goals --file newtext.md
    --if-match <etag>` round-trips cleanly.
- Wrong etag → exit 2 with the mismatch message.
- cargo test + clippy + doc clean.