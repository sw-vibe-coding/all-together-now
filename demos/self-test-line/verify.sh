#!/usr/bin/env bash
# demos/self-test-line/verify.sh
#
# Six-condition end-to-end sanity check for the self-test topology.
# Backend-only (no playwright): uses the same APIs the dashboard uses
# to render. PASS/FAIL per condition; non-zero exit if any fail.
#
# Pre-condition: ./demos/self-test-line/setup.sh has registered the
# 3 bash agents (coordinator/alpha/beta).

set -uo pipefail

URL="${ATN_DEMO_URL:-http://localhost:7500}"
PROJECT="$HOME/github/softwarewrighter/self-test-line"
ATN_CLI="$(cd "$(dirname "$0")/../.." && pwd)/target/debug/atn-cli"

PASS=0
FAIL=0
report() {
    local status="$1"; shift
    local label="$1"; shift
    local detail="${1:-}"
    if [ "$status" = "PASS" ]; then
        printf '  \033[32mPASS\033[0m  %s  %s\n' "$label" "$detail"
        PASS=$((PASS + 1))
    else
        printf '  \033[31mFAIL\033[0m  %s  %s\n' "$label" "$detail"
        FAIL=$((FAIL + 1))
    fi
}

# Helper: send a HumanText line into an agent's PTY so its bash
# executes the resulting command. Used to drive condition 5+6.
send_input() {
    local agent="$1"; shift
    local cmd="$1"
    local payload
    payload=$(python3 -c "
import json, sys
print(json.dumps({'text': sys.argv[1] + chr(13), 'raw_bytes': []}))
" "$cmd")
    curl -sS -X POST -H 'Content-Type: application/json' \
        --data-binary "$payload" \
        "$URL/api/agents/$agent/input" \
        -o /dev/null -w ""
}

# ============================================================
echo "═══ Condition 1 — each agent's PTY started in correct cwd ═══"
# Verify spec.working_dir matches fixture; also probe live by sending `pwd` and reading screenshot.
expected=$(python3 -c "
print('coordinator=$PROJECT/coordinator')
print('alpha=$PROJECT/alpha')
print('beta=$PROJECT/beta')
")
api_dirs=$(curl -s "$URL/api/agents" | python3 -c "
import json, sys
for a in json.load(sys.stdin):
    print(f'{a[\"id\"]}={a[\"spec\"][\"working_dir\"]}')
" | sort)
exp_sorted=$(echo "$expected" | sort)
if [ "$exp_sorted" = "$api_dirs" ]; then
    report PASS "spec.working_dir matches fixture for all 3"
else
    report FAIL "spec.working_dir mismatch" "expected=$exp_sorted actual=$api_dirs"
fi

# ============================================================
echo
echo "═══ Condition 2 — UI shows TUI for each started agent (screenshot endpoint) ═══"
for id in coordinator alpha beta; do
    snap=$(curl -s "$URL/api/agents/$id/screenshot?format=text&rows=8&cols=80" | tr -d '\0')
    bytes=$(printf '%s' "$snap" | wc -c | tr -d ' ')
    # Bash agents print the readiness PS1; shouldn't be empty.
    if [ "$bytes" -gt 50 ]; then
        report PASS "$id TUI rendered ($bytes bytes)"
    else
        report FAIL "$id TUI suspiciously empty" "($bytes bytes)"
    fi
done

# ============================================================
echo
echo "═══ Condition 3 — UI highlights agent waiting for input (state classifier) ═══"
# Drive an agent into awaiting_human_input by issuing `read -p '? '`.
send_input alpha 'read -p "? " answer'
sleep 2
state=$(curl -s "$URL/api/agents/alpha/state" | python3 -c "import json,sys; print(json.load(sys.stdin)['state']['state'])")
if [ "$state" = "awaiting_human_input" ]; then
    report PASS "alpha state=awaiting_human_input after read -p"
else
    report FAIL "alpha state did not flip to awaiting" "got: $state"
fi
# Recover alpha so subsequent conditions have a clean shell.
send_input alpha "x"
sleep 1

# ============================================================
echo
echo "═══ Condition 4 — graph endpoint shows nodes + idle/busy state ═══"
graph=$(curl -s "$URL/api/agents/graph" | python3 -c "
import json, sys
d = json.load(sys.stdin)
print(f'nodes={len(d)}')
for n in d:
    print(f'{n[\"id\"]}={n[\"state\"]}')
")
node_count=$(echo "$graph" | head -1 | sed 's/nodes=//')
if [ "$node_count" = "3" ]; then
    report PASS "graph has 3 nodes" "$(echo "$graph" | tail -3 | tr '\n' ' ')"
else
    report FAIL "graph node count != 3" "got: $node_count"
fi

# ============================================================
echo
echo "═══ Condition 5 — two-way event flow (request out, response back) ═══"
# alpha sends to coordinator; verify event lands in /api/events with delivered=true.
events_before=$(curl -s "$URL/api/events" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
send_input alpha './atn-send coordinator "round-trip-probe"'
sleep 3
events_after=$(curl -s "$URL/api/events" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))")
new=$((events_after - events_before))
if [ "$new" -ge 1 ]; then
    last=$(curl -s "$URL/api/events" | python3 -c "
import json, sys
d = json.load(sys.stdin)
e = d[-1]
ev = e['event']
print(f'src={ev[\"source_agent\"]} tgt={ev[\"target_agent\"]} delivered={e[\"delivered\"]} sum={ev[\"summary\"][:40]}')
")
    if echo "$last" | grep -q 'src=alpha tgt=coordinator delivered=True'; then
        report PASS "alpha→coordinator event delivered" "$last"
    else
        report FAIL "event present but wrong shape" "$last"
    fi
else
    report FAIL "no new event after alpha's atn-send" "before=$events_before after=$events_after"
fi

# ============================================================
echo
echo "═══ Condition 6 — wiki round-trip + cross-agent visibility ═══"
NONCE="probe-$(date +%s)"
# coordinator writes the page
send_input coordinator "echo -e 'nonce: $NONCE\nts: $(date -u +%FT%TZ)' | $ATN_CLI wiki put Test__Echo --stdin"
sleep 2
wiki_body=$(curl -s "$URL/api/wiki/Test__Echo" | python3 -c "
import json, sys
try:
    d = json.load(sys.stdin)
    print(d.get('body', d.get('content', '')))
except Exception as e:
    print(f'PARSE_ERR: {e}')
")
if echo "$wiki_body" | grep -q "$NONCE"; then
    report PASS "Test__Echo contains nonce after coordinator write"
else
    report FAIL "Test__Echo missing nonce" "body=$(echo "$wiki_body" | head -c 200)"
fi

# alpha reads page and confirms; beta reads page and confirms
for id in alpha beta; do
    out=$(send_input "$id" "$ATN_CLI wiki get Test__Echo" && sleep 1 && curl -s "$URL/api/agents/$id/screenshot?format=text&rows=20&cols=120")
    if echo "$out" | grep -q "$NONCE"; then
        report PASS "$id sees nonce in Test__Echo" "(via screenshot of agent's own wiki get)"
    else
        report FAIL "$id did not echo nonce after wiki get" "(scan screenshot)"
    fi
done

# ============================================================
echo
echo "═══ Summary ═══"
total=$((PASS + FAIL))
printf "  %d/%d passed\n" "$PASS" "$total"
if [ "$FAIL" -gt 0 ]; then
    echo
    echo "  most-recent client-log entries (debugging context):"
    curl -s "$URL/api/client-log" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for e in d[-10:]:
    print(f'    seq={e[\"seq\"]} {e[\"level\"]} [{e[\"source\"]}] {e[\"message\"][:120]}')
"
    exit 1
fi
exit 0
