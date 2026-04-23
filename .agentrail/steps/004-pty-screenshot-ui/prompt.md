## Step 4: 📸 snapshot button in window chrome

Add a per-window snapshot affordance. One click opens the snapshot
in a new tab so the user can copy/paste the text or keep it open
for reference.

### Deliverables

1. New `📸` icon button in each window's chrome header, between
   `▸ config` and `↻ reconnect`. Same `.btn-icon` styling as its
   siblings. `data-window-action=\"snapshot\"`.
2. Click handler opens `/api/agents/<id>/screenshot?format=text&rows=40&cols=120`
   in a new tab (`window.open(url, '_blank')`).
3. `window.snapshotAgent(id)` devtools helper exposed on `window`.
4. Screenshot row in docs/windowed-ui.md's chrome table.
5. Live smoke: click the 📸 on a fake-shim agent → new tab shows
   the expected banner + prompt text.

### Acceptance

- Chrome icon renders for every agent panel.
- Click opens the rendered snapshot in a new tab.
- cargo doc + clippy + tests clean.