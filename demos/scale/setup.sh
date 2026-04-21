#!/usr/bin/env bash
# demos/scale/setup.sh
#
# Populate a running ATN with 21 fake agents that exercise the scale-UI
# treemap at scale:
#
#     1  coordinator  (fake-claude)
#     4  spammers     (fake-agent-profile spammer)   — hot tiles
#     8  quiet        (fake-agent-profile quiet)     — tiny tiles
#     4  periodic     (fake-agent-profile periodic)  — pulsing tiles
#     2  awaiting     (fake-agent-profile awaiting-input) — state boost
#     2  error        (fake-agent-profile error)     — crash then vanish
#
# Each profile is served by tools/fake-agent-profile (argv[1] = profile).
# Nothing beyond bash + curl is required on the host.
#
# Usage
#   # Default: boot a local atn-server on :7500 and populate it.
#   ./demos/scale/setup.sh
#
#   # Reuse an already-running server:
#   ATN_DEMO_SKIP_BOOT=1 ATN_DEMO_URL=http://localhost:7500 \
#     ./demos/scale/setup.sh
#
# Environment
#   ATN_DEMO_URL       — Base URL (default http://localhost:7500).
#   ATN_DEMO_SKIP_BOOT — `1` to reuse a running server.

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

url="${ATN_DEMO_URL:-http://localhost:7500}"
server_pid=""

# Prepend tools/ so fake-agent-profile / fake-claude / ... resolve.
export PATH="$repo_root/tools:$PATH"

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
        echo "scale-demo: $url already reachable — reusing running server"
    else
        echo "scale-demo: booting atn-server (logs at /tmp/atn-scale-server.log)"
        (
            cd "$repo_root"
            ./target/debug/atn-server agents.toml
        ) > /tmp/atn-scale-server.log 2>&1 &
        server_pid=$!
        trap 'if [ -n "$server_pid" ]; then echo "scale-demo: tearing down server pid=$server_pid"; kill "$server_pid" 2>/dev/null || true; fi' EXIT
    fi
fi

wait_for_ready
echo "scale-demo: server ready at $url"

post_agent() {
    local name="$1" role="$2" agent_bin="$3" agent_args="$4"
    local body
    body=$(cat <<EOF
{
    "name": "$name",
    "role": "$role",
    "transport": "local",
    "working_dir": ".",
    "project": "scale-demo",
    "agent": "$agent_bin",
    "agent_args": $(if [ -n "$agent_args" ]; then echo "\"$agent_args\""; else echo "null"; fi)
}
EOF
    )
    local code
    code=$(curl -sS -o /dev/null -w '%{http_code}' \
        -X POST -H 'Content-Type: application/json' \
        --data-binary "$body" \
        "$url/api/agents")
    printf '  %-20s %s\n' "$name" "$code"
}

echo "scale-demo: spawning 21 agents..."

post_agent coord-main coordinator fake-claude ''

for i in 1 2 3 4; do
    post_agent "spammer-0$i" worker fake-agent-profile spammer
done

for i in 1 2 3 4 5 6 7 8; do
    post_agent "quiet-0$i" worker fake-agent-profile quiet
done

for i in 1 2 3 4; do
    post_agent "periodic-0$i" worker fake-agent-profile periodic
done

for i in 1 2; do
    post_agent "awaiting-0$i" worker fake-agent-profile awaiting-input
done

for i in 1 2; do
    post_agent "error-0$i" worker fake-agent-profile error
done

echo
echo "scale-demo: 21 agents up. Open $url and watch the treemap."
echo
echo "Try:"
echo "  • Click any spammer — it becomes the focus panel"
echo "  • Press p to pin the current focus (moves to pin row)"
echo "  • Press /  and type 'quiet' — treemap narrows to quiet agents"
echo "  • Select 'group-by: role' — treemap packs by role"
echo "  • Click Save under Layouts, name it 'quiet-focus'"
echo
echo "To tear down just these demo agents (leaves the server running):"
echo "  for id in coord-main spammer-0{1,2,3,4} quiet-0{1..8} periodic-0{1..4} awaiting-0{1,2} error-0{1,2}; do"
echo "    curl -X DELETE $url/api/agents/\$id"
echo "  done"

if [ -n "$server_pid" ]; then
    echo "scale-demo: Ctrl-C to stop the server launched by this script."
    wait "$server_pid"
fi
