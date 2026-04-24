#!/usr/bin/env bash
# demos/atn-agent/setup.sh
#
# End-to-end atn-agent walkthrough with a real Ollama or with the
# integration-test stub (Demo 12).
#
# Two modes:
#   (a) With Ollama running on :11434 (default) — run the agent
#       against the real model named in $MODEL.
#   (b) --stub — spawn a tiny one-shot HTTP stub that answers
#       /api/chat with canned tool-call responses and drives the
#       agent through a file_write → outbox_send → final-content
#       conversation. No external install required.
#
# Usage
#   ./demos/atn-agent/setup.sh            # real Ollama at :11434
#   ./demos/atn-agent/setup.sh --stub     # stub mode (self-contained)
#   MODEL=llama3:8b ./demos/atn-agent/setup.sh   # override model name

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
mode="${1:-real}"
model="${MODEL:-qwen3:8b}"

atn_dir="$repo_root/.atn-demo-atn-agent"
workspace="$(mktemp -d)"
trap 'rm -rf "$workspace"' EXIT

mkdir -p "$atn_dir/inboxes/demo" "$atn_dir/outboxes/demo"

echo "demo: building atn-agent + atn-cli..."
cargo build -p atn-agent -p atn-cli 2>&1 | tail -n 2

# Seed an inbox message — simulates the router handing the agent
# a task from the coordinator.
inbox="$atn_dir/inboxes/demo/ev-demo.json"
cat > "$inbox" <<'EOF'
{
    "event": {
        "id": "ev-demo",
        "kind": "feature_request",
        "source_agent": "coord",
        "source_repo": ".",
        "target_agent": "demo",
        "issue_id": null,
        "summary": "write a notes.md and tell coord when done",
        "wiki_link": null,
        "priority": "normal",
        "timestamp": "2026-04-24T12:00:00Z"
    },
    "delivered": true,
    "delivered_at": "2026-04-24T12:00:01Z"
}
EOF

if [[ "$mode" == "--stub" ]]; then
    echo "demo: running the agent under cargo test's integration stub"
    echo "      (no Ollama required; reproduces the Demo 12 flow)."
    echo
    cargo test -p atn-agent --test integration -- --nocapture 2>&1 | tail -n 40
    echo
    echo "demo: that's the stub flow. Remove $atn_dir when done."
    exit 0
fi

# Real Ollama. Check the endpoint responds — bail early with a
# helpful message if not.
if ! curl -sf --max-time 2 "http://localhost:11434/api/tags" > /dev/null 2>&1; then
    cat <<'MSG' >&2
demo: Ollama not reachable at http://localhost:11434.
      Start it first (`brew install ollama && ollama serve`) or
      rerun this script with --stub to use the canned integration-test
      flow instead.
MSG
    exit 1
fi

echo "demo: running atn-agent against Ollama ($model, workspace=$workspace)"
echo "      inbox message seeded; agent will exit after processing it."
echo

# --exit-on-empty: one pass, then clean exit (no need for Ctrl-C).
# --allow-shell: let the model run shell if it wants to inspect
# the workspace. Feel free to drop that flag for a tighter sandbox.
"$repo_root/target/debug/atn-agent" \
    --agent-id demo \
    --model "$model" \
    --base-url "http://localhost:11434" \
    --atn-dir "$atn_dir" \
    --workspace "$workspace" \
    --inbox-poll-secs 1 \
    --allow-shell \
    --exit-on-empty

echo
echo "demo: inbox after the run:"
ls "$atn_dir/inboxes/demo/"
echo
echo "demo: outbox after the run:"
ls "$atn_dir/outboxes/demo/" 2>/dev/null || echo "  (empty — model may not have called outbox_send)"
echo
echo "demo: workspace after the run:"
find "$workspace" -type f | head
echo
echo "demo: inspect .atn in the repo root at $atn_dir — remove when done."
