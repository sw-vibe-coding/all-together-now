#!/usr/bin/env bash
# demos/git-sync/setup.sh
#
# End-to-end git-sync-agents walkthrough (Demo 13). Builds the
# one-host subset of docs/uber-use-case.md:
#
#   - bare central remote (central.git)
#   - two agent worktrees (alice, bob), each cloned from central
#   - prs-dir + log-dir
#   - one atn-server  driving /api/prs against central + the prs-dir
#   - one atn-syncd  per worktree (alice, bob)
#
# Drops a marker on both worktrees, waits for the PR JSONs to
# appear, runs `atn-cli prs list` → `prs merge` on each, then
# prints the central log to confirm both commits landed on main.
# Cleans up the workspace + child processes on exit.
#
# Usage
#   ./demos/git-sync/setup.sh
#
# No external dependencies beyond git, curl, and a working `cargo`.

set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

work_root="$(mktemp -d)"
log_dir="$work_root/logs"
mkdir -p "$log_dir"

server_pid=""
alice_pid=""
bob_pid=""

cleanup() {
    set +e
    [[ -n "$alice_pid"  ]] && kill "$alice_pid"  2>/dev/null
    [[ -n "$bob_pid"    ]] && kill "$bob_pid"    2>/dev/null
    [[ -n "$server_pid" ]] && kill "$server_pid" 2>/dev/null
    sleep 0.2
    [[ -n "$alice_pid"  ]] && kill -9 "$alice_pid"  2>/dev/null
    [[ -n "$bob_pid"    ]] && kill -9 "$bob_pid"    2>/dev/null
    [[ -n "$server_pid" ]] && kill -9 "$server_pid" 2>/dev/null
    rm -rf "$work_root"
}
trap cleanup EXIT

central="$work_root/central"
alice="$work_root/alice"
bob="$work_root/bob"
prs_dir="$work_root/prs"
mkdir -p "$prs_dir"

echo "demo: workspace = $work_root"
echo "demo: building atn-server + atn-syncd + atn-cli..."
cargo build -p atn-server -p atn-syncd -p atn-cli 2>&1 | tail -n 2

# 1. Central is a non-bare repo so atn-server can `git merge` into
#    main. denyCurrentBranch=ignore lets the syncd push pr/* refs
#    without complaining about the checked-out branch.
git init --quiet --initial-branch=main "$central"
git -C "$central" config user.email "central@demo"
git -C "$central" config user.name "Central"
git -C "$central" config commit.gpgsign false
git -C "$central" config receive.denyCurrentBranch ignore
echo "central seed" > "$central/README.md"
git -C "$central" add README.md
git -C "$central" commit --quiet -m "init"

# 2. Two agent worktrees, both cloned from central so histories
#    overlap (otherwise `git merge` needs --allow-unrelated-histories).
clone_agent() {
    local name="$1"
    local dest="$2"
    git clone --quiet "$central" "$dest"
    git -C "$dest" config user.email "$name@demo"
    git -C "$dest" config user.name "$name"
    git -C "$dest" config commit.gpgsign false
}

clone_agent alice "$alice"
git -C "$alice" checkout --quiet -b feature
echo "from alice" > "$alice/feature-alice.txt"
git -C "$alice" add feature-alice.txt
git -C "$alice" commit --quiet -m "alice: add feature"

clone_agent bob "$bob"
git -C "$bob" checkout --quiet -b feature-z
echo "from bob" > "$bob/feature-z.txt"
git -C "$bob" add feature-z.txt
git -C "$bob" commit --quiet -m "bob: add feature-z"

# 3. atn-server: holds the prs-dir + central. ATN_PORT=0 → kernel
#    picks a free port; we parse the resolved port from stdout.
echo "demo: launching atn-server..."
echo "[project]
name = \"git-sync-demo\"
log_dir = \"$log_dir\"" > "$work_root/agents.toml"

server_log="$log_dir/atn-server.log"
( cd "$work_root" && \
  ATN_PORT=0 RUST_LOG="atn_server=warn" \
  "$repo_root/target/debug/atn-server" agents.toml \
      --prs-dir "$prs_dir" \
      --central-repo "$central" \
      > "$server_log" 2>&1 ) &
server_pid=$!

# Spin until the server prints `atn-server ready on …`.
port=""
for _ in $(seq 1 50); do
    if line=$(grep -m1 "atn-server ready on" "$server_log" 2>/dev/null); then
        port="${line##*:}"
        break
    fi
    sleep 0.1
done
if [[ -z "$port" ]]; then
    echo "demo: atn-server never started — see $server_log" >&2
    exit 1
fi
base="http://127.0.0.1:$port"
echo "demo: atn-server listening on $base"

# 4. One atn-syncd per worktree. Each polls every second for its
#    own marker. Logs go into $log_dir for inspection.
launch_syncd() {
    local agent="$1"
    local repo="$2"
    "$repo_root/target/debug/atn-syncd" \
        --repo "$repo" \
        --agent-id "$agent" \
        --remote origin \
        --prs-dir "$prs_dir" \
        --poll-secs 1 \
        --verbose \
        > "$log_dir/atn-syncd-$agent.log" 2>&1 &
    echo $!
}

echo "demo: launching atn-syncd (one per worktree)..."
alice_pid=$(launch_syncd alice "$alice")
bob_pid=$(launch_syncd bob "$bob")

# 5. Drop markers on both worktrees.
printf 'summary=alice: feature ready for review\n' > "$alice/.atn-ready-to-pr"
printf 'summary=bob: feature-z ready for review\n' > "$bob/.atn-ready-to-pr"
echo "demo: markers dropped on alice + bob; waiting for syncd to push..."

# 6. Wait for both PR JSONs to appear (max ~10 s).
deadline=$(( $(date +%s) + 12 ))
while (( $(date +%s) < deadline )); do
    if ls "$prs_dir"/alice-feature-*.json   > /dev/null 2>&1 && \
       ls "$prs_dir"/bob-feature-z-*.json > /dev/null 2>&1; then
        break
    fi
    sleep 0.3
done

if ! ls "$prs_dir"/alice-feature-*.json > /dev/null 2>&1; then
    echo "demo: alice PR record never appeared — see $log_dir/atn-syncd-alice.log" >&2
    exit 1
fi
if ! ls "$prs_dir"/bob-feature-z-*.json > /dev/null 2>&1; then
    echo "demo: bob PR record never appeared — see $log_dir/atn-syncd-bob.log" >&2
    exit 1
fi

echo
echo "demo: PR records on disk:"
ls -l "$prs_dir"

# 7. atn-cli view of the registry.
echo
echo "demo: atn-cli prs list (open):"
ATN_URL="$base" "$repo_root/target/debug/atn-cli" prs list --status open

# 8. Merge both. Capture the ids from the JSON filenames so we
#    don't have to grep stdout.
alice_id=$(basename "$(ls "$prs_dir"/alice-feature-*.json | head -1)" .json)
bob_id=$(basename "$(ls "$prs_dir"/bob-feature-z-*.json | head -1)" .json)

echo
echo "demo: merging $alice_id..."
ATN_URL="$base" "$repo_root/target/debug/atn-cli" prs merge "$alice_id" \
    | head -n 12

echo
echo "demo: merging $bob_id..."
ATN_URL="$base" "$repo_root/target/debug/atn-cli" prs merge "$bob_id" \
    | head -n 12

# 9. Confirm the merges landed on central main.
echo
echo "demo: central git log --oneline main:"
git -C "$central" log --oneline main | head -n 8

echo
echo "demo: atn-cli prs list (merged):"
ATN_URL="$base" "$repo_root/target/debug/atn-cli" prs list --status merged

echo
echo "demo: done. Workspace ($work_root) and child processes will"
echo "      be cleaned up on exit."
