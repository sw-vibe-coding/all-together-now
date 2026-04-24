# atn-cli

`atn-cli` is the typed HTTP client for the ATN server. It's the
replacement for the `curl + jq` loops in the demo scripts — every
UI interaction has a subcommand, state polling is a first-class
primitive (`wait`), and every exit code is meaningful.

## Usage

```bash
cargo run -p atn-cli -- --help
# or, after cargo build -p atn-cli:
./target/debug/atn-cli <command> …
```

### Base URL

Resolution order:

1. `--base-url <URL>` flag (wins if given).
2. `ATN_URL` environment variable.
3. `http://localhost:7500` (default).

With `--verbose` the CLI prints the resolved base URL and the
request URL before every call.

### Exit codes

| Code | Meaning                                            |
|------|----------------------------------------------------|
| 0    | Success.                                           |
| 1    | Usage error (invalid flags / missing sources).     |
| 2    | Not found (404 / ETag mismatch / unknown agent).   |
| 3    | HTTP or transport error / `wait` timeout.          |
| 4    | Server error (5xx).                                |

## `agents` — lifecycle + observation

```bash
atn-cli agents list                      # table
atn-cli agents list --format json        # raw array

atn-cli agents state <id>                # table
atn-cli agents state <id> --format json  # full JSON incl. stalled flag

atn-cli agents input  <id> "echo hi"     # atomic text+Enter
echo 'pwd'  | atn-cli agents input <id> --stdin   # multi-line from stdin

atn-cli agents stop      <id>
atn-cli agents restart   <id>
atn-cli agents reconnect <id>
atn-cli agents delete    <id>

atn-cli agents wait <id>                             # default: --state idle
atn-cli agents wait <id> --state any-non-starting    # "agent has spun up"
atn-cli agents wait <id> --state awaiting-input --timeout 30

atn-cli agents screenshot <id>                    # text/plain to stdout
atn-cli agents screenshot <id> --format html      # text/html body
atn-cli agents screenshot <id> --rows 20 --cols 80
```

`agents input` appends `\r` to the text so the PTY commits the line
in a single atomic write (matches the UI's windowed-UI step-6 send
fix). Hyphenated state names (`awaiting-input`, `completed-task`)
work everywhere the server accepts the snake_case form.

## `events` — inter-agent event log

```bash
atn-cli events list                    # table
atn-cli events list --since 50         # entries after index 50
atn-cli events list --format json      # raw entries

atn-cli events send \
  --from worker-hlasm --to coordinator \
  --kind completion_notice \
  --summary "task X done" \
  --priority high
```

Valid kinds: `feature_request`, `bug_fix_request`,
`completion_notice`, `blocked_notice`, `needs_info`,
`verification_request`. Hyphenated aliases (`bug-fix-request`)
also work. Valid priorities: `normal`, `high`, `blocking`.

The event id is auto-generated as `cli-<from>-<epoch-millis>`;
timestamp is RFC 3339. Optional flags: `--issue-id`, `--wiki-link`,
`--source-repo` (defaults to `.`).

## `wiki` — coordination pages

```bash
atn-cli wiki list

atn-cli wiki get Coordination/Goals                        # markdown body
atn-cli --verbose wiki get Coordination/Goals 2>etag.txt    # capture ETag

atn-cli wiki put Coordination/Goals --file newtext.md --if-match "$(…)"
echo "# new" | atn-cli wiki put Coordination/Goals --stdin --if-match "..."

atn-cli wiki delete Coordination/Scratch --if-match "..."
```

The server uses 9-digit SHA ETags and returns **409 Conflict** (not
412) when `If-Match` is missing or stale. atn-cli converts that to
`ETag mismatch for '<title>' — refetch and retry (current ETag:
<etag>)` on stderr + exit 2 — so script loops can branch on `$? ==
2` to refetch and retry.

Creating a brand-new page doesn't require `--if-match`; updating an
existing page does. Titles with slashes (`Coordination/Goals`) pass
through unencoded; the server's route matches the raw wildcard path.

## Script recipes

### Seed three agents + wait for them all to be idle

```bash
for name in worker-a worker-b worker-c; do
  curl -sS -X POST -H 'Content-Type: application/json' \
    -d "{\"name\":\"$name\",\"role\":\"worker\",\"transport\":\"local\",\"working_dir\":\".\",\"project\":\"demo\",\"agent\":\"bash\"}" \
    "$ATN_URL/api/agents" > /dev/null
done

for name in worker-a worker-b worker-c; do
  atn-cli agents wait "$name" --state idle --timeout 10 || exit 1
done
echo "all three idle"
```

### Tail new events since a watermark

```bash
watermark=0
while :; do
  atn-cli events list --since "$watermark" --format json \
    | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d))'
  # (script-side: update watermark to len(list) and sleep.)
  sleep 5
done
```

### Update a wiki page with optimistic-concurrency retry

```bash
while :; do
  etag=$(atn-cli --verbose wiki get Coordination/Goals 2>&1 >/dev/null \
         | awk '/^ETag:/{print $2}')
  printf '# Goals\n\n- updated at %s\n' "$(date -Iseconds)" \
    | atn-cli wiki put Coordination/Goals --stdin --if-match "$etag"
  case $? in
    0) echo "ok"; break ;;
    2) echo "ETag stale, retrying…" ; continue ;;
    *) echo "hard error"; exit 1 ;;
  esac
done
```

## Testing

Every subcommand is covered by unit tests in `crates/atn-cli/src/main.rs`
(18 tests: URL resolution, table formatters, state-match aliasing,
event-kind validation, body-source selection, ETag-conflict exit
contract). An end-to-end integration test in
`crates/atn-cli/tests/integration.rs` boots `atn-server` on an
ephemeral port and drives every subcommand group against it.

```bash
cargo test -p atn-cli         # unit + integration
cargo test -p atn-cli --test integration   # just the end-to-end tour
```

See also:
- [docs/demos-scripts.md § Demo 10](./demos-scripts.md#demo-10--atn-cli-tour) — scripted walkthrough.
- [docs/usage.md](./usage.md) — REST API reference the CLI wraps.
- [docs/windowed-ui.md](./windowed-ui.md) — the browser dashboard.
