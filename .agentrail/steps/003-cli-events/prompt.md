## Step 3: atn-cli — events list + send

### Deliverables

1. `atn-cli events list [--since <index>] [--format json|table]` —
   GET `/api/events[?since=N]`. Table columns: logged_at, kind,
   from → to (or 'broadcast'), decision, delivered, summary (first
   80 chars).
2. `atn-cli events send --from <agent> [--to <agent>] --kind <kind>
    --summary <text> [--priority normal|high|blocking] [--issue-id <id>]
    [--wiki-link <path>]` — build a PushEvent with an auto id
   (`cli-<from>-<millis>`) + RFC3339 timestamp, POST to
   `/api/events`. Validates `--kind` against the PushKind enum
   client-side (`feature_request`, `bug_fix_request`,
   `completion_notice`, `blocked_notice`, `needs_info`,
   `verification_request`).
3. Unit tests for the kind parser and event builder.

### Acceptance

- Running `atn-cli events send --from worker-hlasm --to coordinator
   --kind completion_notice --summary "task X done"` yields an
   entry in `atn-cli events list` within a few seconds (the
   message router's poll cadence).
- Bad `--kind` → usage error, exit 1 with the list of valid kinds.
- cargo test + clippy + doc clean.