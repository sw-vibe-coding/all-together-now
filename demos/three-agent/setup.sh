#!/usr/bin/env bash
# demos/three-agent/setup.sh
#
# Stand up the three-agent topology described in docs/uber-use-case.md
# against a running ATN server:
#
#   coordinator  : local (mighty-mike)           ~/work/atn-demo         / claude
#   worker-hlasm : mosh devh1@queenbee           /home/devh1/work/hlasm  / codex
#   worker-rpg   : mosh devr1@queenbee           /home/devr1/work/rpg-ii / opencode-z-ai-glm-5
#
# Usage
#   # Fake-agent mode (what CI uses — no real claude/codex/opencode needed).
#   # PATH is prepended with tools/ so the `agent` field resolves to
#   # tools/fake-claude etc.
#   ./demos/three-agent/setup.sh
#
#   # Real-agent mode — requires claude/codex/opencode-z-ai-glm-5 on PATH.
#   ATN_DEMO_REAL=1 ./demos/three-agent/setup.sh
#
#   # Talk to an already-running server instead of starting one:
#   ATN_DEMO_SKIP_BOOT=1 ATN_DEMO_URL=http://localhost:7500 \
#     ./demos/three-agent/setup.sh
#
# Environment
#   ATN_DEMO_REAL       — `1` to use real CLIs on PATH, anything else to
#                         prepend `tools/` so fake-* shims resolve first.
#   ATN_DEMO_SKIP_BOOT  — `1` to skip `atn-server` launch and reuse whatever
#                         is already listening at $ATN_DEMO_URL.
#   ATN_DEMO_URL        — Base URL of the server. Default: http://localhost:7500.
#   ATN_DEMO_FIXTURES   — Fixture directory. Default: this script's dir/fixtures.

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

url="${ATN_DEMO_URL:-http://localhost:7500}"
fixtures_dir="${ATN_DEMO_FIXTURES:-$script_dir/fixtures}"
server_pid=""

if [ "${ATN_DEMO_REAL:-0}" = "1" ]; then
    echo "demo: ATN_DEMO_REAL=1 — using real claude/codex/opencode on PATH"
else
    export PATH="$repo_root/tools:$PATH"
    echo "demo: using fake shims (tools/fake-claude etc.); set ATN_DEMO_REAL=1 for real"
fi

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
        echo "demo: booting atn-server (logs at /tmp/atn-demo-server.log)"
        (
            cd "$repo_root"
            ./target/debug/atn-server agents.toml
        ) > /tmp/atn-demo-server.log 2>&1 &
        server_pid=$!
        trap 'if [ -n "$server_pid" ]; then echo "demo: tearing down server pid=$server_pid"; kill "$server_pid" 2>/dev/null || true; fi' EXIT
    fi
fi

wait_for_ready
echo "demo: server ready at $url"

for fixture in coordinator.json worker-hlasm.json worker-rpg.json; do
    path="$fixtures_dir/$fixture"
    if [ ! -f "$path" ]; then
        echo "error: missing fixture $path" >&2
        exit 1
    fi
    echo "--- POST $fixture ---"
    if ! curl -sS --fail-with-body \
        -X POST \
        -H 'Content-Type: application/json' \
        --data-binary "@$path" \
        "$url/api/agents"; then
        echo
        echo "error: POST of $fixture failed" >&2
        exit 1
    fi
    echo
done

echo
echo "demo: three-agent topology running. Open $url to watch them."
echo "demo: Ctrl-C to stop the server (if this script launched it)."
if [ -n "$server_pid" ]; then
    wait "$server_pid"
fi
