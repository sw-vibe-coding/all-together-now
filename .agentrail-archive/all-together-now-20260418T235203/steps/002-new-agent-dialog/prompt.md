## Step 2: New Agent Dialog

Replace the current single free-form "launch command" field with a structured
dialog that captures the parts of a remote agent spawn and lets the server
compose the actual shell command.

### Deliverables

1. Dialog fields:
   - `name` (required, unique id)
   - `role` (coordinator | worker | custom)
   - `transport` (local | mosh | ssh)
   - `host` (required if transport != local)
   - `user` (required if transport != local)
   - `working_dir` (required; path on target machine)
   - `project` (optional label for UI; defaults to basename of working_dir)
   - `agent` (select: claude | codex | opencode | gemini | custom)
   - `agent_args` (optional free-form tail appended to the agent command)
2. Server-side composition:
   - local: `cd <dir> && <agent> <args>`
   - mosh / ssh: `<transport> <user>@<host> -- tmux new-session -A -s atn-<name> 'cd <dir> && <agent> <args>'`
   - Shell-escape all user-supplied fields when composing.
3. `POST /api/agents` accepts the structured payload, stores it, and spawns a
   PTY running the composed command. The stored agent record keeps both the
   structured fields and the derived command for UI display.
4. Dialog available both from the Yew UI and the static HTML dashboard.
5. Client-side validation matches the server-side required-field rules per
   transport.

### Acceptance

- POSTing `{transport:"mosh", user:"devh1", host:"queenbee", working_dir:"/home/devh1/work/hlasm", agent:"codex"}` creates an agent whose launch command matches the documented template.
- POSTing with `transport:"local"` uses the local template and no network binary.
- Missing required fields for the chosen transport return a 400 with a field list.
- New test covers command composition for all three transports.
- `cargo test --workspace` green; clippy clean.
