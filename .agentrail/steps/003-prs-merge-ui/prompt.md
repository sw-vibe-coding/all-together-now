## Step 3: Merge / Reject buttons + conflict modal

Wire the existing `POST /api/prs/{id}/{merge,reject}` routes into
the panel. Optimistic UI; conflict stderr surfaces in a modal.

### Deliverables

1. Detail pane gains two buttons in a footer bar:
   - `Merge` (primary): only enabled when `status === "open"`.
     Disables itself while the POST is in flight.
   - `Reject` (secondary, danger styling).
2. Click handler:
   - `POST /api/prs/{id}/merge` (or `/reject`).
   - On 200: don't manually update — wait for the SSE
     `Updated` event from step 1's broadcast (which runs before
     the response returns) to do the update. Re-enable button
     after one round trip in case the SSE connection is laggy.
   - On 404: toast `"PR no longer exists — dashboard out of
     sync"` + close the detail pane.
   - On 409: open a modal with the parsed `{error, stderr}`
     body. Title: `"Merge conflict on <id>"`. Body: pre-formatted
     stderr (preserve newlines). Single dismiss button. Keep
     the PR card on `open` (no optimistic update happened).
   - On any other non-2xx: toast `"server returned <status>"` +
     stderr in expanded console.log.
3. Toast helper: small bottom-left transient (~3 s) — mirror
   any existing toast pattern; if none exists, a minimal
   `<div role="status">` is fine.
4. Conflict modal styles: borrow from the existing config-editor
   modal if there is one; otherwise minimal `<div>` with
   backdrop. Escape key + click-outside dismiss.
5. Browser-side smoke test (Playwright is already set up
   per the wiki-panel saga — confirm by listing
   `crates/atn-server/tests/`): boot atn-server with a tempdir
   prs-dir + central, drop two PR JSONs, navigate to the
   dashboard, click 📋, click Merge on PR #1, assert
   the card flips to `merged` within 3 s. Skip if Playwright
   isn't already wired (note in the step summary).
6. Pure unit test on the JS module if it's testable (the
   existing dashboard JS isn't yet — skip if so; rely on the
   integration test).

### Acceptance

- Drop two PR records (one mergeable, one set up to conflict
  via overlapping main-branch edit), open the panel, click
  Merge on the clean one — card flips to `merged` in-place.
  Click Merge on the conflicting one — modal appears with
  stderr.
- cargo test workspace + clippy --all-targets -D warnings clean.
