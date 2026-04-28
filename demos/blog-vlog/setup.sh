#!/usr/bin/env bash
# demos/blog-vlog/setup.sh
#
# Boots ATN with one coordinator + three worker agents (blog,
# explainer, short) backed by the kitted softwarewrighter repos:
#
#   coordinator : ~/github/softwarewrighter/atn-coordinator  (created on first run)
#   blog        : ~/github/softwarewrighter/atn-blog
#   explainer   : ~/github/softwarewrighter/atn-explainer
#   short       : ~/github/softwarewrighter/atn-short
#
# Each worker repo already ships an AGENTS.md teaching the agent the
# wiki + atn-send protocol and an `atn-send` script that POSTs to
# /api/events. The coordinator gets one too.
#
# Unlike demos/three-agent/setup.sh, there is no fake-shim mode here.
# fake-claude is a stdin echo loop and cannot run atn-wiki / atn-send.
# This demo requires a real `claude` (or other agent CLI) on PATH that
# can read AGENTS.md and execute shell commands.
#
# Usage
#   ./demos/blog-vlog/setup.sh
#
#   # Talk to an already-running server instead of starting one:
#   ATN_DEMO_SKIP_BOOT=1 ATN_DEMO_URL=http://localhost:7500 \
#     ./demos/blog-vlog/setup.sh
#
#   # Use a different agent CLI for all four agents:
#   ATN_DEMO_AGENT=opencode-z-ai-glm-5 ./demos/blog-vlog/setup.sh
#
# Environment
#   ATN_DEMO_SKIP_BOOT  — `1` to skip atn-server launch and reuse whatever
#                         is already listening at $ATN_DEMO_URL.
#   ATN_DEMO_URL        — Base URL of the server. Default: http://localhost:7500.
#   ATN_DEMO_FIXTURES   — Fixture directory. Default: this script's dir/fixtures.
#   ATN_DEMO_AGENT      — Override the `agent` field in every fixture before
#                         POSTing (e.g. `codex`, `opencode-z-ai-glm-5`).

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

url="${ATN_DEMO_URL:-http://localhost:7500}"
fixtures_dir="${ATN_DEMO_FIXTURES:-$script_dir/fixtures}"
agent_override="${ATN_DEMO_AGENT:-}"
server_pid=""

coordinator_dir="$HOME/github/softwarewrighter/atn-coordinator"
blog_dir="$HOME/github/softwarewrighter/atn-blog"
explainer_dir="$HOME/github/softwarewrighter/atn-explainer"
short_dir="$HOME/github/softwarewrighter/atn-short"

# 1. Sanity-check that the worker repos are present. (atn-coordinator
#    is created automatically below; the others must already exist.)
for d in "$blog_dir" "$explainer_dir" "$short_dir"; do
    if [ ! -d "$d" ]; then
        echo "error: missing worker repo $d" >&2
        echo "       expected the kitted softwarewrighter atn-* repo here." >&2
        exit 1
    fi
done

# 2. Auto-provision the coordinator workspace if absent. The AGENTS.md
#    + atn-send shipped with this demo's parent commit are the source
#    of truth; if the dir already exists we leave it alone.
if [ ! -d "$coordinator_dir" ]; then
    echo "demo: coordinator dir missing — creating $coordinator_dir"
    mkdir -p "$coordinator_dir"
fi
if [ ! -f "$coordinator_dir/AGENTS.md" ]; then
    echo "error: $coordinator_dir/AGENTS.md missing — restore it from this repo's history" >&2
    exit 1
fi
if [ ! -x "$coordinator_dir/atn-send" ]; then
    echo "error: $coordinator_dir/atn-send missing or not executable" >&2
    exit 1
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
        echo "demo: booting atn-server (logs at /tmp/atn-blog-vlog-server.log)"
        (
            cd "$repo_root"
            ./target/debug/atn-server agents.toml
        ) > /tmp/atn-blog-vlog-server.log 2>&1 &
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

for fixture in coordinator.json blog.json explainer.json short.json; do
    post_fixture "$fixtures_dir/$fixture"
done

echo
echo "demo: blog-vlog topology running. Open $url to watch them."
echo "demo: coordinator's AGENTS.md tells it to seed Coordination__Goals,"
echo "      brief blog/explainer/short, and route work via atn-send."
echo "demo: Ctrl-C to stop the server (if this script launched it)."
if [ -n "$server_pid" ]; then
    wait "$server_pid"
fi
