ATN — Dashboard Polish

Two independent pieces of dashboard UX that both need the same
JS/CSS/docs loop:

1. The Events view has grown to the point where scanning a long log
   is tedious. It needs filters + inline detail.
2. The wiki is currently a first-class tab, but agents+coord often
   want to glance at a page without leaving the dashboard. A
   collapsible wiki panel next to the dashboard lets the user keep
   one reference page visible alongside the agents.

## Steps

1. events-filter-chips — filter bar above the Events columns:
   chips for kind (feature_request / bug_fix_request / …),
   delivered/undelivered, a text search box matching summary +
   source_agent + target_agent. Persist the filter state in the
   existing windowed-UI localStorage blob so refresh restores it.

2. events-detail-expand — click an event row → inline expand with
   full JSON, formatted timestamp, linkified wiki_link. `Esc`
   collapses. Escalation banners gain a "jump to event" link that
   scrolls + expands the matching entry.

3. wiki-panel-core — global collapsible right-side wiki panel,
   toggled from a new button in the top bar. Dropdown picks a page
   (populated from `GET /api/wiki`); body renders the `html` field
   from `GET /api/wiki/{title}`. Closes with the same button or
   `Esc`. Pure read-only at this step.

4. wiki-panel-live — 5 s poll + ETag-based change detection (the
   server already emits an `ETag` header). On a change, re-render
   + brief flash animation. Clicking a wiki_link in the Events
   view's detail-expand (from step 2) opens the target page in
   the side panel instead of a new tab, if the panel is open.

5. dashboard-polish-docs — new doc + demo + status rows.
   - `docs/events-view.md` — filter chips, detail expand, how the
     router decisions show up.
   - windowed-ui.md gains a "Wiki side panel" section.
   - `docs/demos-scripts.md` Demo 11 "events view + wiki panel".
   - `docs/status.md`: D1..D5 rows.

## Success metrics

- Typing `worker-hlasm` in the Events filter narrows to entries
  involving that agent.
- A `blocked_notice` event row expands in place to show full JSON.
- The wiki panel opens via top-bar button, picks a page, and stays
  in sync when that page is edited elsewhere (wait 5–10 s).
- cargo test + clippy + doc clean.

## Out of scope

- Wiki edit-in-panel (read-only for this saga; atn-cli already
  covers writes).
- Per-agent wiki attachments (the panel is global for now).
