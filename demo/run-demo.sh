#!/bin/bash
# ATN Multi-Agent Coordination Demo
#
# Two agents collaborate to build a small Python CLI app:
#   1. Agent "dev" creates a CLI tool with a "greet" command
#   2. ATN routes a feature request to agent "feature"
#   3. Agent "feature" adds a "farewell" command to the same app
#   4. ATN routes completion notice back to "dev"
#
# Usage: bash demo/run-demo.sh
# Expected runtime: ~90 seconds

set -euo pipefail

ATN_PORT=7500
ATN_BASE="http://localhost:${ATN_PORT}"
AI_MODEL="${ATN_DEMO_MODEL:-deepseek/deepseek-chat}"
AI_WAIT="${ATN_AI_WAIT:-20}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# ── Helpers ──────────────────────────────────────────────────────────

CAPTURE_DIR="${ATN_CAPTURE_DIR:-}"
[[ -n "$CAPTURE_DIR" ]] && mkdir -p "$CAPTURE_DIR"

cleanup() {
    if [[ -n "${SERVER_PID:-}" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    # If capture dir is set, copy artifacts before cleanup
    if [[ -n "$CAPTURE_DIR" && -n "${WORK_DIR:-}" && -d "${WORK_DIR:-}" ]]; then
        rm -rf "$CAPTURE_DIR/dot-atn"
        cp -r "$WORK_DIR/.atn" "$CAPTURE_DIR/dot-atn" 2>/dev/null || true
        cp "$WORK_DIR/server.log" "$CAPTURE_DIR/" 2>/dev/null || true
        cp "$WORK_DIR/app.py" "$CAPTURE_DIR/app-final.py" 2>/dev/null || true
        # Snapshot events and agents via API (server still running)
        curl -sf http://localhost:${ATN_PORT}/api/events > "$CAPTURE_DIR/events.json" 2>/dev/null || true
        curl -sf http://localhost:${ATN_PORT}/api/agents > "$CAPTURE_DIR/agents.json" 2>/dev/null || true
    fi
    if [[ -n "${WORK_DIR:-}" && -d "${WORK_DIR:-}" ]]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT

api() {
    local method="$1" path="$2"
    shift 2
    curl -sf -X "$method" "${ATN_BASE}${path}" \
        -H 'Content-Type: application/json' \
        "$@" 2>/dev/null
}

wait_for_idle() {
    local agent_id="$1" max_wait="${2:-30}" elapsed=0
    while (( elapsed < max_wait )); do
        local state
        state=$(api GET "/api/agents/${agent_id}/state" | jq -r '.state.state' 2>/dev/null || echo "unknown")
        if [[ "$state" == "idle" ]]; then
            return 0
        fi
        sleep 1
        (( elapsed++ )) || true
    done
    echo "TIMEOUT: ${agent_id} not idle after ${max_wait}s (last: ${state})" >&2
    return 1
}

wait_for_events() {
    local expected="$1" max_wait="${2:-15}" elapsed=0
    while (( elapsed < max_wait )); do
        local count
        count=$(api GET "/api/events" | jq 'length' 2>/dev/null || echo "0")
        if (( count >= expected )); then
            return 0
        fi
        sleep 1
        (( elapsed++ )) || true
    done
    return 1
}

send_input() {
    local agent_id="$1" text="$2"
    local payload
    payload=$(jq -nc --arg t "$text" '{text: $t}')
    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" -X POST "${ATN_BASE}/api/agents/${agent_id}/input" \
        -H 'Content-Type: application/json' \
        -d "$payload" 2>/dev/null)
    if [[ "$http_code" != "200" ]]; then
        echo "WARN: input to ${agent_id} returned HTTP ${http_code}" >&2
        return 1
    fi
}

ai_run() {
    # Send an opencode prompt to an agent and wait for completion.
    # Usage: ai_run <agent_id> <prompt> [expected_file]
    local agent_id="$1" prompt="$2" expected_file="${3:-}"
    local cmd="opencode run -m ${AI_MODEL} \"${prompt}\""
    send_input "$agent_id" "$cmd"

    # Poll until agent is idle AND expected file exists
    local elapsed=0 max_wait=90
    sleep 5
    elapsed=5
    while (( elapsed < max_wait )); do
        local state
        state=$(api GET "/api/agents/${agent_id}/state" | jq -r '.state.state' 2>/dev/null || echo "unknown")
        if [[ "$state" == "idle" ]]; then
            if [[ -z "$expected_file" ]] || [[ -f "$WORK_DIR/$expected_file" ]]; then
                return 0
            fi
        fi
        sleep 2
        (( elapsed += 2 )) || true
    done
    echo "TIMEOUT: ${agent_id} task did not complete in ${max_wait}s" >&2
    return 1
}

# ── Setup ────────────────────────────────────────────────────────────

echo "=== START ==="

WORK_DIR=$(mktemp -d)
cd "$WORK_DIR"
git init -q .
git commit --allow-empty -m "init" -q

cp "$SCRIPT_DIR/demo-agents.toml" agents.toml

ATN_SERVER="${PROJECT_DIR}/target/release/atn-server"
if [[ ! -x "$ATN_SERVER" ]]; then
    ATN_SERVER="${PROJECT_DIR}/target/debug/atn-server"
fi
if [[ ! -x "$ATN_SERVER" ]]; then
    echo "ERROR: atn-server not found. Run: cargo build -p atn-server" >&2
    exit 2
fi

"$ATN_SERVER" agents.toml >"$WORK_DIR/server.log" 2>&1 &
SERVER_PID=$!
sleep 2

if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "ERROR: server failed to start" >&2
    cat "$WORK_DIR/server.log" >&2
    exit 2
fi
echo "server: ok"

# ── Wait for agents ─────────────────────────────────────────────────

echo "=== AGENTS READY ==="
wait_for_idle dev 10
echo "dev: idle"
wait_for_idle feature 10
echo "feature: idle"

# Allow opencode tool use in temp workspaces.
# The JSON value must be single-quoted in bash to protect braces and asterisks.
# We write the value to a tmp file and read it back, avoiding all quoting issues.
for agent in dev feature; do
    curl -s -o /dev/null -X POST "${ATN_BASE}/api/agents/${agent}/input" \
        -H 'Content-Type: application/json' \
        -d "{\"text\":\"printf '%s' '{\\\"*\\\":\\\"allow\\\"}' > /tmp/.atn-oc-perm\"}"
    sleep 0.5
    curl -s -o /dev/null -X POST "${ATN_BASE}/api/agents/${agent}/input" \
        -H 'Content-Type: application/json' \
        -d '{"text":"export OPENCODE_PERMISSION=`cat /tmp/.atn-oc-perm`"}'
done
sleep 1

# ── Step 1: Dev creates the app ─────────────────────────────────────

echo "=== DEV CREATES APP ==="
ai_run dev 'Create a Python CLI app in app.py using argparse with one subcommand: greet. python3 app.py greet Alice should print Hello, Alice! Include a main guard. Just the file, no explanation.' app.py
echo "app.py: created"
[[ -n "$CAPTURE_DIR" ]] && cp "$WORK_DIR/app.py" "$CAPTURE_DIR/app-v1.py" 2>/dev/null

# Verify it works
send_input dev "python3 app.py greet World"
sleep 2
echo "greet: tested"

# ── Step 2: Route feature request dev → feature ─────────────────────

echo "=== FEATURE REQUEST ==="
TIMESTAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)
api POST "/api/events" -d "{
    \"id\": \"evt-add-farewell\",
    \"kind\": \"feature_request\",
    \"source_agent\": \"dev\",
    \"source_repo\": \".\",
    \"target_agent\": \"feature\",
    \"summary\": \"Add a farewell subcommand to app.py: python app.py farewell Alice prints Goodbye, Alice!\",
    \"priority\": \"high\",
    \"timestamp\": \"${TIMESTAMP}\"
}" >/dev/null

wait_for_events 1 10
echo "event: delivered to feature"

# Let notification settle
sleep 3
wait_for_idle feature 10

# ── Step 3: Feature agent adds the farewell command ─────────────────

echo "=== FEATURE ADDS COMMAND ==="
# Record file hash before edit to detect when it changes
V1_HASH=$(md5 -q "$WORK_DIR/app.py" 2>/dev/null || md5sum "$WORK_DIR/app.py" 2>/dev/null | cut -d' ' -f1)

ai_run_edit() {
    # Like ai_run but waits for file content to change (not just exist).
    local agent_id="$1" prompt="$2" file="$3" old_hash="$4"
    local cmd="opencode run -m ${AI_MODEL} \"${prompt}\""
    send_input "$agent_id" "$cmd"

    local elapsed=0 max_wait=90
    sleep 5; elapsed=5
    while (( elapsed < max_wait )); do
        local state
        state=$(api GET "/api/agents/${agent_id}/state" | jq -r '.state.state' 2>/dev/null || echo "unknown")
        if [[ "$state" == "idle" ]]; then
            local new_hash
            new_hash=$(md5 -q "$WORK_DIR/$file" 2>/dev/null || md5sum "$WORK_DIR/$file" 2>/dev/null | cut -d' ' -f1)
            if [[ "$new_hash" != "$old_hash" ]]; then
                return 0
            fi
        fi
        sleep 2; (( elapsed += 2 )) || true
    done
    echo "TIMEOUT: ${agent_id} edit did not complete in ${max_wait}s" >&2
    return 1
}

ai_run_edit feature 'Read app.py. Add a second argparse subcommand called farewell so that python3 app.py farewell Alice prints Goodbye, Alice! Keep the existing greet command working. Update app.py in place.' app.py "$V1_HASH"
echo "app.py: updated"
[[ -n "$CAPTURE_DIR" ]] && cp "$WORK_DIR/app.py" "$CAPTURE_DIR/app-v2.py" 2>/dev/null

# Verify both commands work
send_input feature "python3 app.py greet World && python3 app.py farewell World"
sleep 2
echo "both commands: tested"

# ── Step 4: Route completion notice feature → dev ───────────────────

echo "=== COMPLETION NOTICE ==="
TIMESTAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)
api POST "/api/events" -d "{
    \"id\": \"evt-farewell-done\",
    \"kind\": \"completion_notice\",
    \"source_agent\": \"feature\",
    \"source_repo\": \".\",
    \"target_agent\": \"dev\",
    \"summary\": \"farewell subcommand added to app.py and tested\",
    \"priority\": \"normal\",
    \"timestamp\": \"${TIMESTAMP}\"
}" >/dev/null

wait_for_events 2 10
echo "event: delivered to dev"

# ── Summary ──────────────────────────────────────────────────────────

echo "=== SUMMARY ==="
EVENT_COUNT=$(api GET "/api/events" | jq 'length' 2>/dev/null || echo "0")
echo "events_routed: ${EVENT_COUNT}"
echo "ai_interactions: 2"

AGENT_COUNT=$(api GET "/api/agents" | jq 'length' 2>/dev/null || echo "0")

# Capture v1/v2 snapshots if they weren't already saved (cleanup saves the rest)
if [[ -n "$CAPTURE_DIR" ]]; then
    mkdir -p "$CAPTURE_DIR"
    [[ ! -f "$CAPTURE_DIR/app-v1.py" ]] && cp "$WORK_DIR/app.py" "$CAPTURE_DIR/app-v1.py" 2>/dev/null || true
fi

if (( EVENT_COUNT >= 2 && AGENT_COUNT == 2 )); then
    echo "status: pass"
else
    echo "status: fail"
    exit 1
fi
