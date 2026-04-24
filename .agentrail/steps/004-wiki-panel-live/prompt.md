## Step 4: Wiki panel live updates + events-view cross-link

Wire the panel to stay in sync with the wiki, and let the Events
view's detail-expand jump into it.

### Deliverables

1. ETag-based polling: every 5 s (configurable in source as a
   constant), GET the selected page. Pass the last-seen ETag via
   `If-None-Match` so the server can 304 if unchanged. On a 200
   response with a new ETag, re-render and briefly flash the
   panel body (e.g. 300 ms subtle highlight). On 304, no-op.
2. Pause polling when the panel is closed or the browser tab is
   hidden (visibilitychange listener).
3. The Events view's expanded-row `wiki_link` (step 2) becomes a
   smart link: if the wiki panel is open, clicking the link
   switches the panel to that page AND doesn't open a new tab;
   if the panel is closed, the current "new tab" fallback stands.
4. Guard: if the selected page disappears (404 on poll), clear the
   panel body with a muted "page deleted" message instead of an
   error banner.

### Acceptance

- Edit `Coordination/Goals` via `atn-cli wiki put` → the panel
  re-renders within ~5 s with a flash.
- Close the panel → the poll stops firing (verify via devtools
  Network tab; no more 304s while closed).
- Click an event's `wiki_link` with the panel open → the panel
  switches; no new tab.
- cargo test + clippy + doc clean.