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
echo

echo "═══ Stage 1 — gatherers (parallel) ═══"
send gatherer-iron 'mkdir -p ../inventory && printf "{\"agent\":\"gatherer-iron\",\"items\":{\"iron-ore\":4}}\n" > ../inventory/gatherer-iron.json && ./atn-send coordinator "Done: gathered 4 iron-ore"'
send gatherer-coal 'mkdir -p ../inventory && printf "{\"agent\":\"gatherer-coal\",\"items\":{\"coal\":4}}\n"        > ../inventory/gatherer-coal.json && ./atn-send coordinator "Done: gathered 4 coal"'
wait_for_event_count 2 "iron + coal Done"
echo

echo "═══ Stage 2 — smelter ═══"
send smelter 'mkdir -p ../inventory && printf "{\"agent\":\"smelter\",\"items\":{\"ingot\":4}}\n" > ../inventory/smelter.json && ./atn-send coordinator "Done: smelted 4 ingots"'
wait_for_event_count 3 "smelter Done"
echo

echo "═══ Stage 3 — stamper ═══"
send stamper 'mkdir -p ../inventory && printf "{\"agent\":\"stamper\",\"items\":{\"widget\":4}}\n" > ../inventory/stamper.json && ./atn-send coordinator "Done: stamped 4 widgets"'
wait_for_event_count 4 "stamper Done"
echo

echo "═══ Stage 4 — assembler ═══"
send assembler 'mkdir -p ../output && printf "gadgets: [g1] [g2]\ngizmos: [z1] [z2]\n" > ../output/widget.txt && ./atn-send coordinator "Done: assembled 2 gadgets + 2 gizmos"'
wait_for_event_count 5 "assembler Done"
echo

echo "═══ Verify ═══"
echo "  inventory/:"
ls "$PROJECT/inventory" | sed 's/^/    /'
echo "  widget.txt:"
sed 's/^/    /' "$PROJECT/output/widget.txt"
echo
echo "  /api/events:"
curl -s "$URL/api/events" | python3 -c '
import json, sys
events = json.load(sys.stdin)
print(f"    total={len(events)}, all delivered={all(e.get(\"delivered\") for e in events)}")
for e in events:
    ev = e["event"]
    src = ev["source_agent"]
    tgt = ev["target_agent"]
    summ = ev["summary"][:60]
    print(f"    {src:>14}  →  {tgt:<14}  {summ}")
'