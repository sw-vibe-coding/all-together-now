## Step 2: PR panel UI (read-only) + `/api/prs/{id}/diff`

Mirror the wiki side-panel pattern. Right-edge drawer that opens
from a header button, lists all PRs, and click-expands to a
detail panel with the commit diff.

### Deliverables

1. New backend route `GET /api/prs/{id}/diff` in
   `crates/atn-server/src/prs.rs`. Runs
   `git -C <central-repo> show <pr.commit> --stat` and
   `git show <pr.commit>` (or `git diff <merge-base>..<pr.commit>`),
   returns
   `{ commit, stat, diff }` as JSON. Caps `diff` at
   `PR_DIFF_MAX = 256 KiB` with `truncated: true` + `notice` on
   overflow. 404 if the id is unknown; 500 with stderr if the
   git command fails.
2. New JS module in `crates/atn-server/static/index.html` (or a
   sibling include — match wherever the wiki panel lives):
   - `class PrsPanel` mirroring the wiki-panel API: `open()`,
     `close()`, `toggle()`, `refresh()`, `subscribe()`.
   - `subscribe()` opens a single `EventSource('/api/prs/stream')`,
     handles the four `PrsEvent` variants — `Snapshot` rebuilds
     the list, `Created`/`Updated` upsert the card, `Removed`
     deletes it.
   - Card layout: `[id]  [agent]  [branch → target]  [status badge]
     [summary]`. Status badge colour-coded (green=merged,
     amber=open, slate=rejected).
   - Click → detail pane below the list: `key: value` lines for
     every record field (mirrors `atn-cli prs show`), then a
     `Diff ▾` collapsible that lazy-loads `/api/prs/{id}/diff`
     on first expand. Pretty-format the diff with monospace +
     line wrapping; render `+` / `-` lines with subtle
     foreground colours (no full syntax highlighting yet).
3. Header toolbar gets a 📋 button next to 📖 (wiki). Click
   toggles the panel. Persist open/closed state in
   `localStorage` (`atn-prs-panel-open-v1`).
4. Empty-state copy (`(no prs yet — run atn-syncd or drop a
   PrRecord JSON in <prs-dir>)`).
5. CSS lives next to the wiki-panel CSS — same drawer width,
   resizer (if the wiki panel has one), header pattern. No
   pixel-perfect copy needed; visual consistency is enough.
6. Unit test for the diff route handler (uses the
   `fixture_repo()` helper from `prs.rs::tests` — push a branch,
   ask for the diff, assert `stat` contains the file name and
   `diff` contains the added line). No browser-side test for
   step 2; that comes in step 3 with merge.

### Acceptance

- Open the dashboard, click 📋, see the panel slide in.
- Cards reflect the prs-dir state; create / update / remove
  events update the list live (verify by `touch
  <prs-dir>/foo.json` with hand-written content).
- Click a card, see the metadata + a Diff ▾ that lazy-loads.
- cargo test workspace + clippy --all-targets -D warnings + doc
  clean.
