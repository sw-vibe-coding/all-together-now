## Step 5: Dashboard-polish docs + Demo 11 + status

Close the saga with the usual docs pass.

### Deliverables

1. New `docs/events-view.md` — filter chips, text search, detail
   expand, router-decision column, escalation banner + jump link.
2. `docs/windowed-ui.md` gains a "Wiki side panel" section
   covering the top-bar toggle, page picker, ETag-based live
   refresh, and the events-row cross-link behavior.
3. `docs/demos-scripts.md` Demo 11 "Events view + wiki panel"
   with setup / steps / cleanup that:
   - seeds 2 fake agents
   - sends 3–5 events of varying kinds (so chips are meaningful)
   - demonstrates the filter + detail expand
   - opens the wiki panel, edits `Coordination/Goals` via
     `atn-cli wiki put`, and watches it flash.
   "Picking one for a short slot" section gains "Dashboard polish:
   Demo 11 — 5 min".
4. `docs/status.md`: new `D1..D5` rows.

### Acceptance

- Docs readable end-to-end.
- `cargo test --workspace` stays green; `cargo doc --workspace --no-deps`
  warning-free; `cargo clippy --workspace -- -D warnings` clean.