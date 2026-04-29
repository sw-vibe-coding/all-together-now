#!/usr/bin/env bash
# demos/blog-vlog/kickoff.sh
#
# Sends one prompt to the coordinator's PTY to start the demo.
# Coordinator reads its AGENTS.md, briefs the three workers via
# ./atn-send, and goes quiet. Workers reply via ./atn-send when
# done; the message router types those replies back into the
# coordinator's PTY. No polling, no token-burn loops.
#
# Usage
#   ./demos/blog-vlog/kickoff.sh
#   ./demos/blog-vlog/kickoff.sh "Custom project topic in one sentence."
#
# Environment
#   ATN_DEMO_URL  — Base URL of the running atn-server. Default: http://localhost:7500.

set -euo pipefail

url="${ATN_DEMO_URL:-http://localhost:7500}"
default_topic='How four ATN agents (claude coordinator + codex blog + opencode explainer + codex short) coordinate via the message router and shared wiki to produce three deliverables — a blog post, an explainer video, and a short — about ATN itself.'
topic="${1:-$default_topic}"

if ! curl -sS --max-time 2 "$url/api/agents" > /dev/null; then
    echo "error: atn-server not reachable at $url" >&2
    echo "       run ./demos/blog-vlog/setup.sh first" >&2
    exit 1
fi

# Verify the coordinator agent is registered.
if ! curl -sS --max-time 2 "$url/api/agents" \
    | python3 -c 'import json,sys; sys.exit(0 if any(a.get("id")=="coordinator" for a in json.load(sys.stdin)) else 1)'; then
    echo "error: no coordinator agent registered at $url/api/agents" >&2
    exit 2
fi

prompt="Project topic: ${topic}

Read ./AGENTS.md. Then follow it: seed Coordination__Goals + Coordination__Agents, send one ./atn-send to each of blog, explainer, and short with their task, and stop. Do not poll. The framework will type each worker's Done back into your PTY when ready."

# POST the prompt + carriage return to coordinator's PTY input.
# /api/agents/{id}/input takes {text} and HumanText-injects it into
# the agent's PTY (same channel as keyboard typing).
payload="$(python3 -c '
import json, sys
print(json.dumps({"text": sys.argv[1] + "\r", "raw_bytes": []}))
' "$prompt")"

echo "demo: kicking off coordinator with topic:"
printf '       %s\n' "$topic"
echo

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
echo "demo: brief delivered. Watch the dashboard:"
echo "      - coordinator panel: shows the brief + ./atn-send calls"
echo "      - blog/explainer/short panels: receive task prompts via the router"
echo "      - Events tab: every routed event"
echo "      - Wiki tab → Coordination__Log: workers' append-only timeline"
echo
echo "      Coordinator will report SESSION COMPLETE when all three workers"
echo "      have replied with Done. No further input from you needed."
