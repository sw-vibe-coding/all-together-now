#!/usr/bin/env bash
# demos/windowed-ui/setup.sh
#
# Stand up a 4-agent local fleet (1 coordinator + 3 workers) configured
# for the **windowed-UI** walkthrough (Demo 9 in docs/demos-scripts.md).
# Identical topology to demos/claude-opencode/setup.sh, but the post-
# message points at layouts / pin / keyboard instead of the legacy
# treemap.
#
# Usage
#   # Default: fake shims via tools/ — no real LLM CLIs needed.
#   ./demos/windowed-ui/setup.sh
#
#   # Real CLIs from PATH (needs claude + opencode installed).
#   ATN_DEMO_REAL=1 ./demos/windowed-ui/setup.sh
#
#   # Reuse an already-running server.
#   ATN_DEMO_SKIP_BOOT=1 ATN_DEMO_URL=http://localhost:7500 \
#     ./demos/windowed-ui/setup.sh
#
# Environment
#   ATN_DEMO_REAL       — `1` uses real CLIs on PATH; default uses fake shims.
#   ATN_DEMO_SKIP_BOOT  — `1` skips atn-server launch and posts against
#                         $ATN_DEMO_URL (default http://localhost:7500).
#   ATN_DEMO_URL        — Base URL of the server.

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

url="${ATN_DEMO_URL:-http://localhost:7500}"
server_pid=""

if [ "${ATN_DEMO_REAL:-0}" = "1" ]; then
    coordinator_agent="claude"
    worker_agent="opencode"
    echo "demo: ATN_DEMO_REAL=1 — using real claude / opencode on PATH"
else
    export PATH="$repo_root/tools:$PATH"
    coordinator_agent="fake-claude"
    worker_agent="fake-opencode-glm5"
    echo "demo: using fake shims (fake-claude, fake-opencode-glm5); set ATN_DEMO_REAL=1 for real"
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
        echo "demo: booting atn-server (logs at /tmp/atn-windowed-ui.log)"
        (
            cd "$repo_root"
            ./target/debug/atn-server agents.toml
        ) > /tmp/atn-windowed-ui.log 2>&1 &
        server_pid=$!
        trap 'if [ -n "$server_pid" ]; then echo "demo: stopping server pid=$server_pid"; kill "$server_pid" 2>/dev/null || true; fi' EXIT
    fi
fi

wait_for_ready
echo "demo: server ready at $url"

post_agent() {
    local name="$1" role="$2" agent_bin="$3"
    local body
    body=$(cat <<EOF
{
    "name": "$name",
    "role": "$role",
    "transport": "local",
    "working_dir": ".",
    "project": "windowed-ui",
    "agent": "$agent_bin"
}
EOF
    )
    local code
    code=$(curl -sS -o /dev/null -w '%{http_code}' \
        -X POST -H 'Content-Type: application/json' \
        --data-binary "$body" \
        "$url/api/agents")
    printf '  %-16s %s\n' "$name" "$code"
}

echo "demo: spawning 4 agents..."
post_agent coord       coordinator "$coordinator_agent"
post_agent worker-1    worker      "$worker_agent"
post_agent worker-2    worker      "$worker_agent"
post_agent worker-3    worker      "$worker_agent"

cat <<'EOF'

demo: 4 agents up. Open http://localhost:7500 to play with them.

Try the windowed-UI model:

  1. Top bar — click **Stack** to collapse everything except the
     coordinator into the bottom dock. Click any dock cell to swap it
     into the primary slot. Click **Carousel** to see prev/next peeks
     (◀ / ▶ to cycle). Click **Tiled** to return to the grid.
  2. Window chrome — each panel header has [ _ ] [ □ ] [ 📌 ] [ ▸ ]
     [ ↻ ] [ ✕ ]. Click the 📌 on a worker: it turns amber and locks
     in place; switching layouts now leaves it put until you unpin.
  3. Keyboard — click a window's header (accent outline). Press `m`
     to minimize, `M` to maximize, `p` to pin, `1..9` to jump by
     sort order, `←/→` to cycle, `Esc` to restore/deselect. Click
     inside an xterm to route keys to the PTY (amber "typing to PTY"
     badge appears near Send).
  4. Sort — toggle **Name / Recent** in the top bar. Recent sorts by
     smoothed bytes/sec; pump a worker (Send `ping` a few times) to
     see it hop.
  5. Persistence — change layout/sort/pin/selection, hard-refresh
     the browser, and watch the state come back.

For the full walkthrough: docs/windowed-ui.md + docs/demos-scripts.md
Demo 9.

To tear down just these agents (leaves the server running):
  for id in coord worker-1 worker-2 worker-3; do
    curl -X DELETE http://localhost:7500/api/agents/$id
  done
EOF

if [ -n "$server_pid" ]; then
    echo
    echo "demo: Ctrl-C to stop the server this script launched."
    wait "$server_pid"
fi
