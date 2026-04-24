## Step 3: Global wiki side-panel (read-only)

Add a collapsible right-side wiki panel to the dashboard so a
coordinator page (or any other wiki page) can sit alongside the
agent windows without leaving the dashboard tab.

### Deliverables

1. New top-bar button `[📖 Wiki panel]` that toggles
   `.wiki-side-panel` visibility. Collapsed: zero-width (no
   viewport overhead). Expanded: fixed ~340 px on the right side,
   overlapping the dashboard (NOT re-flowing the window grid, to
   avoid stampeding the layout state).
2. Panel content:
   - Header: title of the selected page + close button `[✕]`.
   - Dropdown / select populated from `GET /api/wiki` — picks the
     page. Default: `Coordination/Goals` when present, else the
     first page.
   - Body: renders the `html` field from `GET /api/wiki/{title}`
     into a scrollable region (the server already renders
     markdown → HTML with safe anchor handling).
3. Panel visibility + selected page persist in `atn-window-ui-v1`
   under a new `wikiPanel: { open: bool, title: string }` sub-key.
4. `Esc` (when focus is on the dashboard and no input is active)
   closes the panel if the layout manager doesn't already claim it.
   Otherwise clicking the `✕` closes it.

### Acceptance

- Click the top-bar button → the panel slides in from the right
  with the default page loaded.
- Pick a different page from the dropdown → content swaps.
- Hard refresh restores panel state + last-selected page.
- cargo test + clippy + doc clean.