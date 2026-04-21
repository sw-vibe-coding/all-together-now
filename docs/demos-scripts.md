# ATN Demo Scripts

Every demo you can run against the current build (through scale-UI step 5).
Each section is self-contained — pick the one that fits your audience and
time window.

## Prerequisites shared by every demo

- Build the workspace once:
  ```bash
  cargo build --workspace
  ```
- The fake agent shims in `tools/` are already executable. If you want
  them on `$PATH` (used by the three-agent demo script), export:
  ```bash
  export PATH="$(pwd)/tools:$PATH"
  ```
- The server's default port is 7500. Override with `ATN_PORT` if 7500 is
  already in use on your machine.

## At-a-glance index

| # | Demo                                    | Duration | Needs real rack host? |
|---|-----------------------------------------|----------|-----------------------|
| 1 | Empty-start + New Agent dialog (local)  | 3 min    | No                    |
| 2 | New Agent dialog preview — mosh/ssh     | 2 min    | No                    |
| 3 | Three-agent topology (fake shims)       | 4 min    | No                    |
| 4 | Reconnect after mosh drop               | 5 min    | **Yes** (or substitute) |
| 5 | Graceful delete clears remote tmux      | 2 min    | **Yes**               |
| 6 | Treemap scale-UI (heat + focus + pin + kbd) | 6 min | No                    |
| 7 | REST API tour                           | 4 min    | No                    |

What is **not** demoable yet: a fleet of ~20 fake agents exercising the
treemap at scale (saga step 8 `scale-demo-docs`), the `/`-search + role
filter chips + group-by-role packing (saga step 7
`search-filter-groups`), and the Ollama / CUDA transports (future
sagas).

---

## Demo 1 — Empty start + New Agent dialog (local)

**What it shows**
- ATN boots with zero agents; the dashboard renders a deliberate empty
  state with a `+ New Agent` call-to-action.
- The dialog captures a structured `SpawnSpec` (not a free-form shell
  command) and renders a live preview of the composed shell line.

**Why it matters**
- The dialog is the safest way to spawn an agent: fields are validated
  per transport, forbidden characters are rejected, and the exact
  command ATN types into the PTY is visible before you click Create.

**Setup**
```bash
cargo run -p atn-server
```
Open http://localhost:7500.

**Steps**
1. Dashboard shows **No agents yet** + a primary **+ New Agent** button.
2. Click **+ New Agent**. Fill:
   - `name`: `demo-1`
   - `role`: `worker`
   - `transport`: `local`
   - `working_dir`: `.`
   - `agent`: `bash`
3. Watch the preview line update as you type: `cd . && bash`.
4. Click **Create**. A terminal panel appears with a live bash shell.
5. Hover the agent name in the header — tooltip shows the same composed
   command ATN typed into the PTY.

**Cleanup**
Click the agent's **Delete** control, or:
```bash
curl -X DELETE http://localhost:7500/api/agents/demo-1
```

**Variations**
- Create a second agent with `agent=fake-claude` to see the banner a
  real LLM CLI would emit on startup.
- Try invalid input (blank name) — the **Create** button stays disabled
  and the missing-field list appears.

---

## Demo 2 — New Agent dialog, mosh/ssh preview

**What it shows**
- Switching `transport` to `mosh` or `ssh` instantly rewrites the
  preview to the full pipeline:
  ```
  mosh <user>@<host> -- tmux new-session -A -s atn-<name> 'cd <dir> && <agent>'
  ```
- The `-A` flag is how reconnect (Demo 4) re-attaches without losing
  work.

**Why it matters**
- Audience sees, at dialog time, the exact tmux pipeline. Everyone
  understands what will run and where.

**Setup**
Same as Demo 1. Don't click Create — this demo is about the preview.

**Steps**
1. Click **+ New Agent**.
2. Switch `transport` to `mosh`. Notice `user`/`host` become required.
3. Fill: `name: worker-hlasm`, `user: devh1`, `host: queenbee`,
   `working_dir: /home/devh1/work/hlasm`, `agent: codex`.
4. Preview reads:
   ```
   mosh devh1@queenbee -- tmux new-session -A -s atn-worker-hlasm \
     'cd /home/devh1/work/hlasm && codex'
   ```
5. Switch `transport` to `ssh` — the binary flips but everything else
   is identical.
6. Click **Cancel**.

**Cleanup**
Nothing to clean — no agent was created.

---

## Demo 3 — Three-agent topology (fake shims)

**What it shows**
- The canonical multi-agent topology from
  [docs/uber-use-case.md](./uber-use-case.md) stood up from an empty
  ATN with three `POST /api/agents` calls:
  - coordinator (local, `claude`)
  - worker-hlasm (mosh / `codex`)
  - worker-rpg (mosh / `opencode-z-ai-glm-5`)
- For CI / demo without a real remote, `tools/fake-*` shims stand in
  for the real CLIs.

**Why it matters**
- Proves the architecture end-to-end (PTY + router + wiki + events)
  without needing real agent CLIs or a remote host.
- The integration test
  `crates/atn-server/tests/three_agent_demo.rs` exercises the same
  topology in CI.

**Setup**
```bash
# In one terminal:
cargo run -p atn-server
# In another (script boots/reuses the server and posts fixtures):
./demos/three-agent/setup.sh
```

**Steps**
1. Dashboard fills with three panels. Coordinator auto-picks focus.
2. Each fake shim prints an identifying banner then loop-echoes stdin
   with a per-agent prefix.
3. Send a message from coordinator to worker-hlasm via the API:
   ```bash
   curl -X POST -H 'Content-Type: application/json' \
        -d '{
          "id":"demo-3-req",
          "kind":"feature_request",
          "source_agent":"coordinator",
          "source_repo":".",
          "target_agent":"worker-hlasm",
          "summary":"please wire the glue",
          "priority":"normal",
          "timestamp":"2026-04-21T18:00:00Z"
        }' \
        http://localhost:7500/api/events
   ```
4. Within ~2 s the worker-hlasm panel shows:
   `[ATN task from coordinator] please wire the glue`
5. Click the **Events** tab to see both the outbound + inbound log.

**Cleanup**
Ctrl-C the script's server, or:
```bash
for id in coordinator worker-hlasm worker-rpg; do
  curl -X DELETE http://localhost:7500/api/agents/$id
done
```

**Variations**
- `ATN_DEMO_REAL=1 ./demos/three-agent/setup.sh` runs against real
  CLIs (requires `claude` on PATH + `queenbee` reachable via mosh).
- Edit the fixtures in `demos/three-agent/fixtures/` to point at your
  own rack host.

---

## Demo 4 — Reconnect after mosh drop

**What it shows**
- Killing the local `mosh` process flips the agent state to
  **Disconnected** but the remote `tmux` session survives.
- Clicking **Reconnect** hard-kills the dead local child and respawns
  the composed command; because it uses `tmux new-session -A`, the
  respawn re-attaches to the still-running session and in-progress
  agent work is intact.

**Why it matters**
- For long-running agents (compiling a repo, streaming tokens from an
  LLM), a network blip shouldn't throw away progress.

**Setup — option A (real queenbee)**
```bash
ATN_DEMO_REAL=1 ./demos/three-agent/setup.sh
```
Needs `ssh devh1@queenbee` configured with keys, `tmux` on queenbee,
and the agent CLIs (or a fake script) installed as `devh1`.

**Setup — option B (substitute host)**
Any machine you can reach via `mosh` and run `tmux` on works. Adjust
the fixture JSON accordingly.

**Steps**
1. Dashboard shows worker-hlasm panel streaming.
2. On queenbee, confirm the session exists:
   ```bash
   sudo -u devh1 tmux list-sessions   # atn-worker-hlasm
   ```
3. From the Mac:
   ```bash
   pkill -f 'mosh .* devh1@queenbee'
   ```
4. Dashboard: worker-hlasm output stops; state chip flips to
   **Disconnected**.
5. On queenbee again: `tmux list-sessions` still lists the session.
6. Click **Reconnect** on the worker-hlasm panel (or
   `POST /api/agents/worker-hlasm/reconnect`).
7. Output resumes from where it left off.

**Cleanup**
Click **Delete** on the agent — Demo 5 picks up from here.

**Variations**
- Pull your ethernet / turn off wifi for 30 s instead of `pkill` to
  simulate a real blip.
- Reconnect via the API:
  ```bash
  curl -X POST http://localhost:7500/api/agents/worker-hlasm/reconnect
  ```

---

## Demo 5 — Graceful delete clears remote tmux

**What it shows**
- `DELETE /api/agents/{id}` for a remote agent first sends
  `Ctrl-B :kill-session Enter` over the PTY so tmux cleans up its
  server-side session before the local `mosh` child is reaped.

**Why it matters**
- Without this, deleting many agents over time would accumulate
  orphaned `atn-*` sessions on the rack host.

**Setup**
You need a remote agent running. Easiest: continue from Demo 4, or
spawn one via `ATN_DEMO_REAL=1 ./demos/three-agent/setup.sh`.

**Steps**
1. On queenbee: `sudo -u devh1 tmux list-sessions` → `atn-worker-hlasm`
   is listed.
2. On the Mac, click **Delete** on worker-hlasm's panel (or
   `curl -X DELETE http://localhost:7500/api/agents/worker-hlasm`).
3. On queenbee: `sudo -u devh1 tmux list-sessions` → `no server
   running on /tmp/tmux-*/default` (or the session is gone).

**Cleanup**
Nothing to do — the delete path is the cleanup.

---

## Demo 6 — Treemap scale-UI

**What it shows** (scale-UI saga, steps 1–5)
- Per-agent **heat** score (EWMA of bytes-per-second + state
  multipliers) drives tile area in a squarified treemap.
- Focus panel auto-picks (coordinator first, else highest heat) and
  occupies 40–70% of viewport width.
- Click any tile's header or a compact tile to swap it into focus;
  layout freezes for 5 s so you can read.
- Xterm renders at fixed native geometry (120×40); tile resizing is
  pure CSS `transform: scale()`. PTY is never resized by layout
  churn — only an explicit focus.
- Pin row holds up to 6 always-visible mini tiles; pinned agents skip
  the treemap.
- Keyboard: `1..9` focus Nth hottest, `0` coordinator, `f` toggle
  focus width, `p` pin/unpin, `Esc` clear focus, `/` (filter input,
  stub until step 7 lands).
- Pins + focus + focus width persist in localStorage across refreshes.

**Why it matters**
- The dashboard scales from 3 to ~30 agents without the grid turning
  into unreadable micro-panels. Hot agents naturally command attention.

**Setup**
```bash
cargo run -p atn-server
```
Open http://localhost:7500. No agents yet.

**Steps — spawn a mix by hand**
1. Create 6–10 agents via the dialog. Use a mix of transports (all
   `local` is fine for a demo) and agents:
   - 1 coordinator / `fake-claude`
   - 3 workers / `fake-claude` (quiet)
   - 2 workers / `bash` (you can poke them to pump bytes)
   - 2 workers / `fake-codex` (quiet)
2. Observe: coordinator auto-picks focus (big left panel); workers
   tile to the right proportional to (very small) heat.
3. Click into one of the bash agents' input box and send several
   lines — watch its tile grow in the treemap as bytes/sec climbs.
   After 5 s of quiet, it shrinks again.
4. Hover an agent name — tooltip shows the composed launch command.
5. Click a non-focus tile's header → it swaps to focus; layout
   freezes 5 s.
6. Press `1` — the Nth hottest tile is focused.
7. Press `p` — the current focus is pinned (📌 on header, moves to
   pin row, focus returns to auto-pick).
8. Press `f` — focus panel toggles medium (40–50%) ↔ large (55–70%).
9. Press `Esc` — focus clears; auto-pick takes over on the next tick.
10. Refresh the page — pins and focus are restored from localStorage.

**Setup — scripted mix (faster)**
The three-agent script (Demo 3) plus a couple of extras gets you to
~5 agents quickly:
```bash
./demos/three-agent/setup.sh
# In another shell, add a few more:
for i in 1 2 3; do
  curl -X POST -H 'Content-Type: application/json' \
    -d "{\"name\":\"extra-$i\",\"transport\":\"local\",\"working_dir\":\".\",\"agent\":\"fake-claude\"}" \
    http://localhost:7500/api/agents
done
```

**Cleanup**
```bash
for id in $(curl -s http://localhost:7500/api/agents | python3 -c \
  "import json,sys;print(' '.join(a['id'] for a in json.load(sys.stdin)))"); do
  curl -X DELETE http://localhost:7500/api/agents/$id
done
```

**Variations**
- Run the fake-claude agents with `pump-input` loops to keep their
  heat up:
  ```bash
  for i in $(seq 1 50); do
    curl -sS -X POST -H 'Content-Type: application/json' \
      -d "{\"text\":\"ping $i\"}" \
      http://localhost:7500/api/agents/extra-1/input
    sleep 0.3
  done
  ```
- Check raw numbers via `GET /api/agents/heat`.

---

## Demo 7 — REST API tour

**What it shows**
- Every UI interaction is backed by a REST endpoint. Scripts and bots
  get the same surface as the browser.

**Why it matters**
- Integrations (CI, monitoring, custom dashboards) don't need to
  scrape the UI.

**Setup**
```bash
cargo run -p atn-server
```

**Steps**
```bash
# List agents (empty).
curl -s http://localhost:7500/api/agents

# Create one from a SpawnSpec.
curl -s -X POST -H 'Content-Type: application/json' \
  -d '{"name":"api-demo","transport":"local","working_dir":".","agent":"fake-claude"}' \
  http://localhost:7500/api/agents | python3 -m json.tool

# Heat + sparkline fuel.
curl -s http://localhost:7500/api/agents/heat | python3 -m json.tool

# Send an event (appears in /api/events, and if targeted, in the
# target's inbox + TUI).
curl -s -X POST -H 'Content-Type: application/json' \
  -d '{
    "id":"evt-demo",
    "kind":"feature_request",
    "source_agent":"api-demo",
    "source_repo":".",
    "summary":"hello",
    "priority":"normal",
    "timestamp":"2026-04-21T18:00:00Z"
  }' \
  http://localhost:7500/api/events

# Event log.
curl -s http://localhost:7500/api/events | python3 -m json.tool

# Wiki list.
curl -s http://localhost:7500/api/wiki | python3 -m json.tool

# Reconnect (no-op for local, but returns 200).
curl -s -X POST http://localhost:7500/api/agents/api-demo/reconnect

# Delete.
curl -s -X DELETE http://localhost:7500/api/agents/api-demo
```

**Cleanup**
Delete any lingering agents (the last curl above does it).

**Variations**
- Point the [docs/usage.md](./usage.md) REST table at the running
  server and walk it top-to-bottom.

---

## Picking one for a short slot

- **Under 5 min, no infra**: Demo 1 + the preview bit of Demo 2.
- **5–10 min, no infra**: Demo 3 + the middle of Demo 6 (type into a
  bash agent, watch its tile grow).
- **10–15 min, real rack host**: Demos 1 → 2 (preview) → 4 (reconnect)
  → 5 (cleanup). That's the full uber-use-case story.
- **Integrations / API-minded audience**: Demo 7 + a click-through of
  Demo 6.

## When new demos arrive

- Saga step 7 `search-filter-groups` ships: add a demo for `/`-filter
  and role chips.
- Saga step 8 `scale-demo-docs` ships: add a "fleet of 20 agents"
  demo exercising the treemap under load.
- Future Ollama / CUDA / git-sync sagas: each gets its own section.

See also:
- [docs/usage.md](./usage.md) — operational guide
- [docs/uber-use-case.md](./uber-use-case.md) — design story
- [docs/demo-three-agent.md](./demo-three-agent.md) — the three-agent
  demo in more depth
- [docs/remote-pty.md](./remote-pty.md) — manual remote PTY
  walkthrough
