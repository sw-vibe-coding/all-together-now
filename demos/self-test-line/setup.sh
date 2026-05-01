#!/usr/bin/env bash
# demos/self-test-line/setup.sh
#
# Boot ATN with one coordinator + two workers (alpha, beta), all
# running bash so the AGENTS.md scripts are deterministically
# scriptable. The full sanity-check checklist (cwd, TUI, awaiting,
# graph, two-way events, wiki ack chain) is asserted by verify.sh.
#
# Usage:
#   ./demos/self-test-line/setup.sh                  # boot server + register all 3
#   ATN_DEMO_SKIP_BOOT=1 ./demos/self-test-line/setup.sh   # against running server

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

url="${ATN_DEMO_URL:-http://localhost:7500}"
fixtures_dir="${ATN_DEMO_FIXTURES:-$script_dir/fixtures}"
server_pid=""

project_root="$HOME/github/softwarewrighter/self-test-line"

for d in coordinator alpha beta; do
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
        echo "demo: booting atn-server (logs at /tmp/atn-self-test-line-server.log)"
        (
            cd "$repo_root"
            ./target/debug/atn-server agents.toml
        ) > /tmp/atn-self-test-line-server.log 2>&1 &
        server_pid=$!
        trap 'if [ -n "$server_pid" ]; then echo "demo: tearing down server pid=$server_pid"; kill "$server_pid" 2>/dev/null || true; fi' EXIT
    fi
fi

wait_for_ready
echo "demo: server ready at $url"

for fixture in coordinator.json alpha.json beta.json; do
    path="$fixtures_dir/$fixture"
    echo "--- POST $(basename "$path") ---"
    if ! curl -sS --fail-with-body \
        -X POST \
        -H 'Content-Type: application/json' \
        --data-binary "@$path" \
        "$url/api/agents"; then
        echo
        echo "error: POST of $path failed" >&2
        exit 1
    fi
    echo
done

echo
echo "demo: self-test-line topology running. next: ./demos/self-test-line/verify.sh"
if [ -n "$server_pid" ]; then
    wait "$server_pid"
fi
