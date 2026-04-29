#!/usr/bin/env bash
# demos/factory-line/setup.sh
#
# Boots ATN with one coordinator + five workers (gatherer-iron,
# gatherer-coal, smelter, stamper, assembler) running a Factorio-style
# production chain. Goal: 2 gadgets + 2 gizmos.
#
# Single project dir at ~/github/softwarewrighter/factory-line/ with
# one subdir per agent and shared inventory/ + output/ dirs.
#
# Usage:
#   ./demos/factory-line/setup.sh
#   ATN_DEMO_SKIP_BOOT=1 ATN_DEMO_URL=http://localhost:7500 \
#     ./demos/factory-line/setup.sh
#   ATN_DEMO_AGENT=bash ./demos/factory-line/setup.sh   # smoke test wiring

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

url="${ATN_DEMO_URL:-http://localhost:7500}"
fixtures_dir="${ATN_DEMO_FIXTURES:-$script_dir/fixtures}"
agent_override="${ATN_DEMO_AGENT:-}"
server_pid=""

project_root="$HOME/github/softwarewrighter/factory-line"

# 1. Sanity-check the project layout.
for d in coordinator gatherer-iron gatherer-coal smelter stamper assembler; do
    if [ ! -d "$project_root/$d" ]; then
        echo "error: missing $project_root/$d" >&2
        exit 1
    fi
    if [ ! -f "$project_root/$d/AGENTS.md" ]; then
        echo "error: missing $project_root/$d/AGENTS.md" >&2
        exit 1
    fi
    if [ ! -x "$project_root/$d/atn-send" ]; then
        echo "error: missing or non-executable $project_root/$d/atn-send" >&2
        exit 1
    fi
done

# 2. Reset shared inventory + output so the demo starts clean.
rm -f "$project_root/inventory/"*.json "$project_root/output/"*.txt 2>/dev/null || true
mkdir -p "$project_root/inventory" "$project_root/output"

wait_for_ready() {
    for _ in $(seq 1 50); do
        if curl -s --max-time 1 "$url/api/agents" > /dev/null; then
            return 0
        fi
        sleep 0.2
    done
    echo "error: $url/api/agents never responded" >&2
    return 1
}

if [ "${ATN_DEMO_SKIP_BOOT:-0}" != "1" ]; then
    if curl -s --max-time 1 "$url/api/agents" > /dev/null; then
        echo "demo: $url already reachable — reusing running server"
    else
        echo "demo: booting atn-server (logs at /tmp/atn-factory-line-server.log)"
        (
            cd "$repo_root"
            ./target/debug/atn-server agents.toml
        ) > /tmp/atn-factory-line-server.log 2>&1 &
        server_pid=$!
        trap 'if [ -n "$server_pid" ]; then echo "demo: tearing down server pid=$server_pid"; kill "$server_pid" 2>/dev/null || true; fi' EXIT
    fi
fi

wait_for_ready
echo "demo: server ready at $url"

post_fixture() {
    local path="$1"
    local payload
    if [ -n "$agent_override" ]; then
        payload="$(python3 -c "
import json, sys
with open('$path') as f:
    spec = json.load(f)
spec['agent'] = '$agent_override'
print(json.dumps(spec))
")"
    else
        payload="$(cat "$path")"
    fi
    echo "--- POST $(basename "$path") ---"
    if ! curl -sS --fail-with-body \
        -X POST \
        -H 'Content-Type: application/json' \
        --data-binary "$payload" \
        "$url/api/agents"; then
        echo
        echo "error: POST of $path failed" >&2
        exit 1
    fi
    echo
}

for fixture in coordinator.json gatherer-iron.json gatherer-coal.json smelter.json stamper.json assembler.json; do
    post_fixture "$fixtures_dir/$fixture"
done

echo
echo "demo: factory-line topology running. Open $url to watch them."
echo "demo: next: ./demos/factory-line/kickoff.sh"
echo "demo: Ctrl-C to stop the server (if this script launched it)."
if [ -n "$server_pid" ]; then
    wait "$server_pid"
fi
