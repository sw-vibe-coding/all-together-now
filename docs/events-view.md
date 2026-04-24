# Events view

The **Events** tab shows the push-event log the message router
maintains — agent↔agent requests, completion notices, blocked
notices, and escalations. It's the dashboard's audit trail for the
coordination plane.

## Layout

- **Escalation banners** at the top — one per escalated event (the
  router couldn't route to a named target). Always visible, never
  filtered.
- **Two columns** below:
  - **Coordinator → Agents** (outbound): events where `target_agent`
    is anything other than `coordinator`.
  - **Agents → Coordinator** (inbound): events where `target_agent`
    is `coordinator`.

Each event is a card with the kind chip, router decision, summary,
`from`/`target` line, and a short `HH:MM:SS` timestamp. Clicking
expands the card — see [Detail expand](#detail-expand).

## Filter bar

Above the two columns:

- **Text search** (`Filter by summary or agent id…`): case-insensitive
  substring match across `summary`, `source_agent`, and
  `target_agent`.
- **Kind chips** — six toggle-chips for the `PushKind` variants:
  `feature`, `bug-fix`, `completed`, `blocked`, `needs-info`,
  `verify`. Chips **OR within the category** — toggle two to see
  events of either kind.
- **Delivered radio** — `all` / `delivered` / `undelivered`. A
  delivered event is one the router handed to a live PTY; broadcasts
  and unknown-target escalations read as undelivered.
- **✕ reset** — clears text + chips + sets delivered to `all`.
- **N / M entries** — right-aligned count. Shows `M entries` when no
  filter is active and `N / M entries` when narrowed.

Filter state persists in `localStorage` under
`atn-window-ui-v1`'s `eventsFilter` sub-key, so a hard refresh keeps
the last-used filter. Escalation banners are **not** filtered — the
"needs attention" lane shouldn't disappear when you narrow the scan
lane.

## Detail expand

Click any event card to expand it in place. The detail panel shows:

| Row          | Source                                                |
|--------------|-------------------------------------------------------|
| Event ID     | `PushEvent.id`                                        |
| Timestamp    | `PushEvent.timestamp` as local time + `Xs/m/h/d ago`  |
| Priority     | `normal` / `high` / `blocking`                        |
| Source repo  | `PushEvent.source_repo`                               |
| Issue id     | `PushEvent.issue_id` if set                           |
| Wiki link    | `PushEvent.wiki_link` rendered as a clickable anchor  |
| Delivered    | yes / no                                              |
| Decision     | router decision string (`deliver:<id>`, `broadcast`, `escalate:<reason>`) |

A `<pre>` block below the table shows the full `EventLogEntry` as
pretty-printed JSON.

Only one card expands at a time — clicking a second collapses the
first. Pressing `Esc` collapses the current expansion (guarded by
`isTypingTarget` + `isXtermFocused` so it doesn't interfere with the
[Option-C window-management](./windowed-ui.md#keyboard-option-c)
`Esc` or a focused xterm).

Expansion survives refreshes — if the card is still visible under
the current filter after `eventsRefresh()`, it stays open. If the
filter hid it, the expansion clears.

### Wiki link reuse

When the **wiki panel** (see [windowed-ui.md § Wiki side
panel](./windowed-ui.md#wiki-side-panel)) is open, clicking an
event's wiki link reuses that panel — the panel switches to the
referenced page instead of opening a new browser tab. When the
panel is closed, the link falls through to `target="_blank"` and
opens `/wiki/<title>` in a new tab.

## Escalation → jump to event

Each escalation banner carries a **`jump to event ▸`** button on
the right. Clicking it scrolls the matching event card into view
(smooth, centered) and expands it. Useful when a long log buries
the specific event that triggered the escalation.

## REST surface

The filter + detail work is entirely client-side on top of the
existing REST shape. No new endpoints.

```bash
# Full log.
curl -s http://localhost:7500/api/events

# Just new entries since the last scan (useful for scripts).
curl -s http://localhost:7500/api/events?since=42

# Submit an event (atn-cli equivalent: `atn-cli events send …`).
curl -sS -X POST -H 'Content-Type: application/json' \
  -d '{"id":"ev-1","kind":"completion_notice",
       "source_agent":"worker-hlasm","source_repo":".",
       "target_agent":"coordinator","summary":"task X done",
       "priority":"normal","timestamp":"2026-04-24T12:00:00Z"}' \
  http://localhost:7500/api/events
```

See also:
- [docs/windowed-ui.md](./windowed-ui.md) — dashboard layout,
  keyboard, wiki side panel.
- [docs/atn-cli.md § events](./atn-cli.md#events--inter-agent-event-log)
  — CLI reference for `events list` + `events send`.
- [docs/demos-scripts.md § Demo 11](./demos-scripts.md#demo-11--events-view--wiki-panel)
  — scripted walkthrough of filter + detail + wiki-panel live
  update + cross-link.
