## Step 1: Events view filter chips + text search

Scanning the Events view becomes hard once there are dozens of
entries. Add a filter bar above the two-column layout.

### Deliverables

1. New `.events-filter-bar` above the existing
   `.events-toolbar` / `.events-columns`:
   - Text search input (`#events-search`) — matches summary
     (case-insensitive substring) and agent ids (source + target).
   - Kind chips: feature_request / bug_fix_request /
     completion_notice / blocked_notice / needs_info /
     verification_request. Click-toggle; AND across chips is
     ignored (single OR set within the kind category).
   - Delivered toggle: `all` / `delivered` / `undelivered`.
   - Clear-filters button ("✕ reset") and a per-filter count on
     the far right ("N / M entries").
2. Filter state persists in the existing `atn-window-ui-v1`
   localStorage blob under a new `eventsFilter` sub-key.
3. Applied in the refresh loop that populates the outbound /
   inbound columns — don't duplicate the refresh logic; filter
   in-place before rendering.
4. No new endpoints — the filter is client-side on top of
   `GET /api/events`.

### Acceptance

- Typing `worker-hlasm` narrows the columns to entries involving
  that agent. Clearing the input restores the full log.
- Toggling the `blocked_notice` chip hides every other kind.
- Hard refresh restores the prior filter state.
- cargo test + clippy + doc clean. (No Rust changes expected;
  verify the workspace stays green.)