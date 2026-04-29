#!/usr/bin/env bash
# demos/factory-line/kickoff.sh
#
# Sends one prompt to the coordinator's PTY to start the production
# chain. Coordinator briefs gatherers in parallel, then smelter →
# stamper → assembler in sequence as Done messages arrive.
#
# Usage:
#   ./demos/factory-line/kickoff.sh
#   ./demos/factory-line/kickoff.sh "build 3 gadgets and 1 gizmo"

set -euo pipefail

url="${ATN_DEMO_URL:-http://localhost:7500}"
goal="${1:-build 2 gadgets and 2 gizmos}"

if ! curl -sS --max-time 2 "$url/api/agents" > /dev/null; then
    echo "error: atn-server not reachable at $url" >&2
    echo "       run ./demos/factory-line/setup.sh first" >&2
    exit 1
fi

if ! curl -sS --max-time 2 "$url/api/agents" \
    | python3 -c 'import json,sys; sys.exit(0 if any(a.get("id")=="coordinator" for a in json.load(sys.stdin)) else 1)'; then
    echo "error: no coordinator agent registered at $url/api/agents" >&2
    exit 2
fi

prompt="Goal: ${goal}.

Read ./AGENTS.md and follow it. Compute totals, seed Production__Goals once via atn-cli wiki put, dispatch gatherers in parallel, then smelter → stamper → assembler as each Done message arrives in your PTY. Print SESSION COMPLETE when assembler reports Done."

payload="$(python3 -c '
import json, sys
print(json.dumps({"text": sys.argv[1] + "\r", "raw_bytes": []}))
' "$prompt")"

echo "demo: kicking off coordinator with goal:"
printf '       %s\n\n' "$goal"

if ! curl -sS --fail-with-body \
    -X POST \
    -H 'Content-Type: application/json' \
    --data-binary "$payload" \
    "$url/api/agents/coordinator/input"; then
    echo
    echo "error: failed to POST input to coordinator" >&2
    exit 3
fi

echo
echo "demo: brief delivered. Watch the dashboard."
echo "      Expected sequence:"
echo "        1. coordinator atn-cli wiki put Production__Goals"
echo "        2. coordinator ./atn-send gatherer-iron + gatherer-coal (parallel)"
echo "        3. gatherer-iron + gatherer-coal Done (in any order)"
echo "        4. coordinator ./atn-send smelter"
echo "        5. smelter Done"
echo "        6. coordinator ./atn-send stamper"
echo "        7. stamper Done"
echo "        8. coordinator ./atn-send assembler"
echo "        9. assembler Done — writes ../output/widget.txt"
echo "       10. coordinator prints SESSION COMPLETE"
echo
echo "      Inventory files:  $HOME/github/softwarewrighter/factory-line/inventory/*.json"
echo "      Final output:     $HOME/github/softwarewrighter/factory-line/output/widget.txt"
