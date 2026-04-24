## Step 2: atn-cli — agents input/stop/restart/wait/screenshot

Round out the `agents` subcommand with the write-path primitives
every demo script needs.

### Deliverables

1. `atn-cli agents input <id> <text>` — POSTs
   `{"text": "<text>\r"}` to `/api/agents/{id}/input`. Matches the
   atomic text+enter shape the UI uses (see windowed-UI step 6 send
   fix), so a `pwd` command round-trips correctly through a bash
   agent.
2. `atn-cli agents input <id> --stdin` — read the full prompt text
   from stdin and POST it the same way. Lets scripts pipe multi-line
   inputs without bash-escaping.
3. `atn-cli agents stop <id>` / `atn-cli agents restart <id>` /
   `atn-cli agents reconnect <id>` / `atn-cli agents delete <id>` —
   thin POST/DELETE wrappers. 404 → exit 2.
4. `atn-cli agents wait <id> [--state idle|running|awaiting-input|
    any-non-starting] [--timeout 30] [--poll-interval 500ms]` —
   poll `/api/agents/{id}/state` with exponential backoff capped at
   `--poll-interval * 4`. Exit 0 on match, non-zero on timeout.
   Default state: `idle`. Document the canonical state strings in
   `--help`.
5. `atn-cli agents screenshot <id> [--format text|ansi|html]
    [--rows N] [--cols N]` — GET `/api/agents/{id}/screenshot?...`,
   pipe the body to stdout. Respect the endpoint's Content-Type
   (text/plain vs text/html); no transformation in the CLI.
6. Unit tests for the wait-state backoff calculator and the
   state-match predicate (e.g. `any-non-starting`).

### Acceptance

- `atn-cli agents input stuck 'pwd'` followed by
  `atn-cli agents wait stuck --state idle --timeout 5` returns 0
  within a few seconds when run against a bash agent.
- `atn-cli agents screenshot stuck | head -5` prints the tail of
  the agent's terminal.
- cargo test + clippy + doc clean.