## Step 4: Status chips + agent filter + free-text search

Bring the events-view filter pattern over to the PR panel. The
prs-dir grows monotonically; without filters the list will be
unmanageable after a few sessions.

### Deliverables

1. Filter row above the card list:
   - Status chips: `All` / `Open` / `Merged` / `Rejected`.
     Multi-select (click-toggle), default = `Open`. Counts in
     parens (`Open (3)`).
   - Agent dropdown: derived from the records, sorted lexically.
     Default `All`. The dropdown rebuilds when the SSE feed
     introduces a new agent.
   - Free-text search input: filters on summary + branch + id
     (case-insensitive substring). 300 ms debounce.
2. Filtering is purely client-side — no extra API calls. The
   panel keeps the full list internally and re-renders the
   filtered view.
3. Persist all three filter values in `localStorage` under
   `atn-prs-filter-v1`. Restore on panel open.
4. Empty filtered list shows
   `(0 of N PRs match the current filter — clear filters)` with
   a "clear filters" button.
5. Counter in the panel header: `"K of N PRs"` (mirrors the
   events-view counter).
6. Browser-side smoke (if Playwright wired up): drop three
   records (one open / one merged / one rejected), open panel,
   click `Merged` chip, assert only one card visible. Click
   `All`, assert three. Type a substring in search, assert the
   filtered count updates.
7. Unit tests where possible — if the JS lives in a module
   that's loaded by `index.html`, factor the predicate
   (`record_matches(record, filter)`) so it can be exercised by
   `wasm-pack test` if the project has it, or as a small
   doc-test if not. If no test infra exists, document and rely
   on the integration test from step 3.

### Acceptance

- Filter chips + dropdown + search visibly narrow the list and
  persist across reload.
- cargo test workspace + clippy --all-targets -D warnings clean.
