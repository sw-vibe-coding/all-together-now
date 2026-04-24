## Step 4: shell_exec + outbox_send + inbox_ack tools

Round out the tool kit: bounded shell exec (behind a flag),
outbox writes, and explicit inbox acks.

### Deliverables

1. `shell_exec(command: &str)`:
   - Gated on `--allow-shell`; if the flag is off, the tool
     returns `"shell_exec disabled"` (don't surface as an error —
     the model should be told it can't run shell).
   - Spawn under `/bin/sh -c <command>` with `current_dir =
     workspace_root`. No network sandboxing (out of scope).
   - 30 s timeout via a thread + channel (keeps things sync).
     Returns a concatenated stdout+stderr, truncated to 4 KiB
     with a `"…truncated"` marker if exceeded. Also surfaces
     the exit code in the tool result JSON.
2. `outbox_send(target: String, kind: String, summary: String)`:
   - Validates `kind` against the same `PushKind` enum atn-cli
     uses (`feature_request` … `verification_request`).
   - Constructs a `PushEvent` with an auto `agent-<id>-<millis>`
     id, RFC3339 timestamp, `priority: normal` default,
     `source_repo: "."`, no `wiki_link` / `issue_id`.
   - Writes the JSON to
     `<atn-dir>/outboxes/<agent-id>/<event-id>.json` —
     same shape the router polls.
3. `inbox_ack(message_id: String)`:
   - Locates `<atn-dir>/inboxes/<agent-id>/<message-id>.json`,
     renames to `.json.done`. Returns `"acked <id>"` or a
     friendly error if the file is missing.
   - The main inbox loop from step 1 still auto-acks after a
     successful chat turn; this tool lets the model explicitly
     ack mid-run (useful when a single prompt references multiple
     inbox messages).
4. Extend the tool registry + schemas to expose all three. Each
   gets a concise `description` string — the model reads these
   to decide when to call.
5. Unit tests:
   - `shell_exec` gated: `--allow-shell` off → "disabled"; on
     → captures stdout (trivial `echo hello`).
   - `outbox_send` writes a parseable PushEvent to the expected
     path; invalid kind returns a typed error.
   - `inbox_ack` renames a sample file; missing file is a clean
     error.

### Acceptance

- With `--allow-shell`, an Ollama prompt like "run `date`" ends
  up producing a tool-call + captured output in the agent PTY.
- A model-driven `outbox_send(..., kind: "completion_notice", ...)`
  shows up in `/api/events` within a couple of router polls.
- cargo test + clippy + doc clean.