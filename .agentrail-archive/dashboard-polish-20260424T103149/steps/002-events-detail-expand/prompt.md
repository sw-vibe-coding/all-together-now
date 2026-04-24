## Step 2: Inline event-row expansion + escalation jump link

Make each event row in the Events view clickable to reveal the full
PushEvent + EventLogEntry payload.

### Deliverables

1. Each `.event-card` becomes click-to-expand. When expanded:
   - Formatted timestamp (user's local tz + relative "2m ago").
   - Full JSON payload in a `<pre>`.
   - `wiki_link` rendered as a clickable hyperlink — opens the
     target page in a new tab for now (step 4 rewires to the
     side-panel when it's open).
   - `issue_id` rendered as muted text (no link plumbing needed).
2. Only one card expanded at a time; clicking another collapses the
   previous. `Esc` collapses the current expansion.
3. Escalation banner (`.escalation-banner`) gains a "jump to event"
   link — scrolls the relevant column to the entry and expands it.
4. The expanded state is NOT persisted (pure UI affordance).

### Acceptance

- Click a `blocked_notice` row → expands inline showing the full
  JSON payload and a `wiki_link` that opens in a new tab.
- Pressing `Esc` collapses it.
- An escalation banner's "jump" link scrolls + expands.
- cargo test + clippy + doc clean.