ATN — atn-cli

A Rust CLI that wraps the ATN HTTP API so demo scripts and
integrations don't have to hand-roll curl + jq. Matches the sketch
in docs/needed-tools.md §1 but adds the step-3 screenshot endpoint.

## Why

- Demo scripts lean on `curl` + `jq` for agent spawn/input/state
  polling. That works but mixes JSON-escaping, status-code checks,
  and argv quoting in shell. A typed CLI collapses that into
  readable commands with clean exit codes.
- Wait-for-state loops (`while state != idle; do sleep; done`) are
  fragile. A `wait` subcommand with structured AgentState parsing,
  timeout, and exponential backoff is the right primitive.
- The screenshot endpoint (ops-polish step 3) needs a consumer —
  `atn-cli agents screenshot <id>` makes it usable from scripts
  without manual URL construction.

## Steps

1. cli-scaffold — new `atn-cli` crate + clap derive + sync HTTP
   client (ureq or reqwest blocking) + `--base-url` flag +
   `ATN_URL` env. First subcommands: `agents list` (table or
   `--format json`) and `agents state <id>`. JSON pretty-print for
   diagnostics; table formatter for humans.

2. cli-agents-actions — input/stop/restart/wait/screenshot:
   - `atn-cli agents input <id> <text>` — POSTs HumanText.
   - `atn-cli agents input <id> --stdin` — read text from stdin.
   - `atn-cli agents stop <id>` / `atn-cli agents restart <id>` —
     POST the corresponding endpoint.
   - `atn-cli agents wait <id> [--state idle|running|awaiting-input]
      [--timeout 30]` — poll with exponential backoff, exit 0 on
     match, non-zero on timeout.
   - `atn-cli agents screenshot <id> [--format text|ansi|html]
      [--rows N] [--cols N]` — fetch screenshot, print to stdout
     (respecting content-type from the endpoint).

3. cli-events — list + send:
   - `atn-cli events list [--since N] [--format json|table]`.
   - `atn-cli events send --from <agent> [--to <agent>] --kind <kind>
      --summary <text> [--priority normal|high|blocking]` — build
     a PushEvent with auto-generated id + RFC3339 timestamp, POST
     to /api/events. Validates the kind enum client-side.

4. cli-wiki — list/get/put/delete + ETag handling:
   - `atn-cli wiki list` — GET /api/wiki.
   - `atn-cli wiki get <title>` — GET /api/wiki/<title> (prints
     body + ETag header to stderr with --verbose).
   - `atn-cli wiki put <title> [--file path | --stdin]
      [--if-match <etag>]` — PUT with the content body.
   - `atn-cli wiki delete <title> [--if-match <etag>]` — DELETE.
   - On 412 Precondition Failed, print a concise
     "ETag mismatch — refetch and retry" message and exit 2.

5. cli-integration-and-docs — integration tests + docs/atn-cli.md +
   Demo 10 in docs/demos-scripts.md + cross-links.
   - `crates/atn-cli/tests/integration.rs` boots a fresh atn-server
     on an ephemeral port, spawns a fake-claude, then exercises
     agents list / input / wait / screenshot + events send + wiki
     get/put end-to-end with `Command::new` against the just-built
     `atn-cli` binary.
   - `docs/atn-cli.md` documents every subcommand with examples
     (replicating the shapes used by the demo scripts).
   - `docs/demos-scripts.md` gains **Demo 10 — atn-cli tour** and
     the "Picking one for a short slot" section gets a cli-first
     option.

## Success metrics

- `atn-cli agents list` matches the web dashboard's view.
- `atn-cli agents wait <id> --state idle --timeout 10` returns 0
  within a few seconds after spawn or non-zero on timeout.
- Integration test boots a real server and passes end-to-end.
- cargo test + clippy + doc warning-free.
- No changes to existing demo scripts in this saga — `atn-cli`
  adoption can happen in a follow-up once the tool has soaked.
