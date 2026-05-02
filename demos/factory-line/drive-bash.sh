#!/usr/bin/env bash
# demos/factory-line/drive-bash.sh
#
# Bash-mode end-to-end test: drives the production line directly via
# /api/agents/<id>/input (no AI). Proves the message router + PTY
# input pipeline + AGENTS.md script bodies all work end-to-end. Each
# stage waits for the prior stage's Done to land before triggering
# the next.
#
# Pre-req: ./demos/factory-line/setup.sh has registered all 6 agents
# with `agent: bash` (use ATN_DEMO_AGENT=bash on setup).
#
# Usage:
#   ./demos/factory-line/drive-bash.sh

set -euo pipefail

URL="${ATN_DEMO_URL:-http://localhost:7500}"
PROJECT="$HOME/github/softwarewrighter/factory-line"

send() {
    local agent="$1"; shift
    local cmd="$1"
    local payload
    payload="$(python3 -c "
import json, sys
print(json.dumps({'text': sys.argv[1] + chr(13), 'raw_bytes': []}))
" "$cmd")"
    curl -sS -X POST -H 'Content-Type: application/json' \
        --data-binary "$payload" \
        "$URL/api/agents/$agent/input" \
        -o /dev/null -w "  → send to $agent: %{http_code}\n"
}

wait_for_event_count() {
    local target_count="$1"
    local label="$2"
    for _ in $(seq 1 20); do
        local got
        got=$(curl -s "$URL/api/events" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))')
        if [ "$got" -ge "$target_count" ]; then
            echo "  ✓ events=$got ($label)"
            return 0
        fi
        sleep 0.5
    done
    echo "  ✗ timeout waiting for $target_count events ($label)"
    return 1
}

echo "═══ Pre-flight ═══"
echo "  url:     $URL"
echo "  project: $PROJECT"
rm -f "$PROJECT/inventory/"*.json "$PROJECT/output/"*.txt 2>/dev/null || true
mkdir -p "$PROJECT/inventory" "$PROJECT/output"
ATN_CLI="$(cd "$(dirname "$0")/../.." && pwd)/target/debug/atn-cli"

# wiki_put: idempotent put. Tries no-etag first (succeeds if page is
# new); on conflict, fetches the ETag via `atn-cli --verbose wiki get`
# (which prints `ETag: "<hex>"` on stderr) and retries with --if-match.
wiki_put() {
    local title="$1"
    local content="$2"
    if printf '%s' "$content" | "$ATN_CLI" wiki put "$title" --stdin > /dev/null 2>&1; then
        return 0
    fi
    local etag
    etag=$( ( "$ATN_CLI" --verbose wiki get "$title" > /dev/null ) 2>&1 \
        | awk -F'"' '/^ETag:/ { print $2; exit }')
    if [ -n "$etag" ]; then
        # The server expects the RFC-7232 quoted form ("hex"), not bare hex.
        # atn-cli passes --if-match through verbatim into the HTTP header.
        printf '%s' "$content" | "$ATN_CLI" wiki put "$title" --stdin --if-match "\"$etag\"" > /dev/null 2>&1 && return 0
    fi
    echo "  ! wiki_put $title failed (etag='$etag')" >&2
    return 1
}

# wiki_append: read current body, concat one line, put-with-etag.
wiki_append() {
    local title="$1"
    local line="$2"
    local cur
    cur=$("$ATN_CLI" wiki get "$title" 2>/dev/null) || cur=""
    wiki_put "$title" "${cur}
- ${line}"
}

echo "═══ Seed wiki: Production__Goals + Production__Log ═══"
wiki_put Production__Goals '# Production goals
- target: 2 gadgets + 2 gizmos
- chain: gather → smelt → stamp → assemble'
wiki_put Production__Log '# Production log'
echo "  ✓ seeded"
echo

echo "═══ Stage 1 — gatherers (parallel) ═══"
send gatherer-iron 'mkdir -p ../inventory && printf "{\"agent\":\"gatherer-iron\",\"items\":{\"iron-ore\":4}}\n" > ../inventory/gatherer-iron.json && ./atn-send coordinator "Done: gathered 4 iron-ore"'
send gatherer-coal 'mkdir -p ../inventory && printf "{\"agent\":\"gatherer-coal\",\"items\":{\"coal\":4}}\n"        > ../inventory/gatherer-coal.json && ./atn-send coordinator "Done: gathered 4 coal"'
wait_for_event_count 2 "iron + coal Done"
# Mirror inventory state into wiki (one page per worker).
wiki_put Inventory__gatherer-iron '{"agent":"gatherer-iron","items":{"iron-ore":4}}'
wiki_put Inventory__gatherer-coal '{"agent":"gatherer-coal","items":{"coal":4}}'
wiki_append Production__Log "[$(date +%H:%M:%S)] gatherer-iron + gatherer-coal Done"
echo "  ✓ wrote Inventory__gatherer-{iron,coal} pages + appended Production__Log"
echo

echo "═══ Stage 2 — smelter ═══"
send smelter 'mkdir -p ../inventory && printf "{\"agent\":\"smelter\",\"items\":{\"ingot\":4}}\n" > ../inventory/smelter.json && ./atn-send coordinator "Done: smelted 4 ingots"'
wait_for_event_count 3 "smelter Done"
wiki_put Inventory__smelter '{"agent":"smelter","items":{"ingot":4}}'
wiki_append Production__Log "[$(date +%H:%M:%S)] smelter Done — 4 ingots"
echo "  ✓ wrote Inventory__smelter + Production__Log update"
echo

echo "═══ Stage 3 — stamper ═══"
send stamper 'mkdir -p ../inventory && printf "{\"agent\":\"stamper\",\"items\":{\"widget\":4}}\n" > ../inventory/stamper.json && ./atn-send coordinator "Done: stamped 4 widgets"'
wait_for_event_count 4 "stamper Done"
wiki_put Inventory__stamper '{"agent":"stamper","items":{"widget":4}}'
wiki_append Production__Log "[$(date +%H:%M:%S)] stamper Done — 4 widgets"
echo "  ✓ wrote Inventory__stamper + Production__Log update"
echo

echo "═══ Stage 4 — assembler ═══"
send assembler 'mkdir -p ../output && printf "gadgets: [g1] [g2]\ngizmos: [z1] [z2]\n" > ../output/widget.txt && ./atn-send coordinator "Done: assembled 2 gadgets + 2 gizmos"'
wait_for_event_count 5 "assembler Done"
wiki_append Production__Log "[$(date +%H:%M:%S)] assembler Done — 2 gadgets + 2 gizmos. SESSION COMPLETE."
echo "  ✓ Production__Log final entry appended"
echo

echo "═══ Verify ═══"
echo "  inventory/:"
ls "$PROJECT/inventory" | sed 's/^/    /'
echo "  widget.txt:"
sed 's/^/    /' "$PROJECT/output/widget.txt"
echo
echo "  /api/events:"
curl -s "$URL/api/events" > /tmp/_drive_bash_events.json
python3 - <<'PY'
import json
events = json.load(open('/tmp/_drive_bash_events.json'))
print(f"    total={len(events)}, all delivered={all(e.get('delivered') for e in events)}")
for e in events:
    ev = e['event']
    src = ev['source_agent']
    tgt = ev['target_agent']
    summ = ev['summary'][:60]
    print(f"    {src:>14}  →  {tgt:<14}  {summ}")
PY