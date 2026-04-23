## Step 6: Search, Filter, Groups

Given 20+ agents, a name-only scan gets old fast. Add filtering on top of
the treemap so the pool of visible tiles can be narrowed down.

### Deliverables

1. `/` focuses a filter input in the header. Typing fuzzy-matches on
   `name`, `project`, `role`, and `spec.host`/`spec.user`. Hits stay
   visible; misses get hidden (removed from the treemap's input list —
   not just opacity-faded).
2. Filter chips: one-click toggles for `transport: local|mosh|ssh`,
   `role: coordinator|worker|qa|pm`, and `state:
   running|awaiting_input|error|disconnected`. Multiple chips combine
   with AND.
3. Grouping toggle: when on, the treemap packs agents by `role` (or
   `project`) — each group gets a bordered region, tiles inside sized by
   heat as before.
4. Saved layouts: a small "Layouts" dropdown in the header. User can
   name the current (pin set + focus + filter chips + group toggle)
   layout and recall it later. Store in localStorage.
5. Filter + pins interact sensibly: pinned tiles are always visible
   regardless of filter, but get a muted "(filtered)" label if the filter
   excludes them.

### Acceptance

- With ~20 fake agents, `/hlasm<Enter>` leaves only matching tiles.
- Toggling role=worker chip hides the coordinator.
- Group toggle packs tiles cleanly with gap lines between groups.
- A saved layout re-applies correctly after browser refresh.
