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
| 6 | Treemap scale-UI (legacy model)         | 6 min    | No                    |
| 7 | REST API tour                           | 4 min    | No                    |
| 8 | Scale-UI fleet, 21 fake agents (legacy) | 5–15 min | No                    |
| 9 | Windowed UI — layouts + pin + keyboard  | 5 min    | No                    |
|10 | atn-cli tour — agents / events / wiki   | 5 min    | No                    |
|11 | Events view polish + wiki side-panel    | 5 min    | No                    |

What is **not** demoable yet: the Ollama / CUDA transports (future
sagas). Everything else in the scale-UI saga (search, chips, grouping,
saved layouts, the 21-agent fleet) is shipped.

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

## Demo 6 — Treemap scale-UI (legacy model)

> This is the legacy scale-UI. The default dashboard is now the
> **windowed UI** — see [Demo 9](#demo-9--windowed-ui) and
> [docs/windowed-ui.md](./windowed-ui.md). The treemap remains useful
> for ~20+ agent fleets (see Demo 8) where heat-sized tile area is
> load-bearing.

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

---

## Demo 8 — Scale-UI fleet, 21 fake agents (legacy model)

> Legacy scale-UI walkthrough — kept because it's still the right
> lens for ~20+ agent fleets where tile-area-by-heat matters. For
> day-to-day usage with a handful of long-lived agents, see
> [Demo 9](#demo-9--windowed-ui).

**What it shows**
- Everything scale-UI saga steps 1–7: heat-sized treemap, click-to-focus
  with 5 s freeze, compact tiles below threshold, pin row, keyboard
  shortcuts, `/` filter + transport/role/state chips, group-by-role
  packing, saved layouts, localStorage persistence.
- Five distinct activity profiles via `tools/fake-agent-profile`:
  spammer (hot), quiet (tiny), periodic (pulsing), awaiting-input
  (state-boosted), error (5 s then Disconnected).

**Why it matters**
- Single command stands up a realistic fleet; the treemap earns its
  keep at 20+ agents where the old cols-N grid ran out of room.

**Setup**
```bash
./demos/scale/setup.sh
```
The script boots (or reuses) `atn-server` on :7500 and POSTs 21 agents
(1 coordinator + 4 spammers + 8 quiet + 4 periodic + 2 awaiting + 2
error).

**Steps**
See [docs/scale-ui.md](./scale-ui.md) for the full guided walkthrough
(8 exercises: click-to-focus, pin row, 1–9 keyboard, `/` filter, chip
interactions, group-by, saved layouts, disconnect visibility). The
short version:

1. Open http://localhost:7500 — treemap fills in within a few seconds.
   Coordinator is focus; spammers dominate the right side.
2. Press `1` to focus the hottest spammer. Press `p` to pin it. Press
   `Esc` to clear focus.
3. Press `/` and type `quiet` — treemap narrows to the 8 quiet agents.
4. Toggle **group-by: role** — watch the treemap pack into labeled
   clusters.
5. Click **Save** under Layouts. Name it `quiet-focus`. Refresh the
   browser — your layout comes back.

**Cleanup**
Ctrl-C the terminal running `setup.sh`, or:
```bash
for id in coord-main spammer-0{1,2,3,4} quiet-0{1,2,3,4,5,6,7,8} \
          periodic-0{1,2,3,4} awaiting-0{1,2} error-0{1,2}; do
    curl -X DELETE http://localhost:7500/api/agents/$id
done
```

**Variations**
- Edit `demos/scale/setup.sh` to change counts or mix in more
  `awaiting-input` agents to see how the state boost dominates the
  treemap.

---

## Demo 9 — Windowed UI

**What it shows** (windowed-UI saga, steps 1–6)
- Three layout modes — **Tiled** (grid), **Stack** (one primary +
  dock), **Carousel** (primary + prev/next peeks) — selectable in
  the top bar. Coord always occupies the prominent slot.
- Per-window chrome (minimize, maximize/restore, pin, config,
  reconnect, delete) and click-to-select with accent-green outline.
- **Pin = lock-in-place**: pinning snapshots the window's current
  rect; subsequent layout ops skip it, so it floats at that rect.
- Sparkline row at the top with one cell per agent (`flex: 1 1 0`,
  ~1/n of width). Click a cell to focus that agent.
- Keyboard (Option C): `m` minimize, `M` maximize/restore, `p` pin,
  `←/→` cycle, `1..9` jump, `Esc` restore-then-deselect. Guarded
  by `isTypingTarget` + `document.activeElement.closest('.xterm')`.
- Persistence in `localStorage` under `atn-window-ui-v1` —
  layoutMode / sortMode / selectedId / per-window state (including
  pinnedRect) survive a hard refresh.

**Why it matters**
- The treemap (Demo 6 + 8) is great at 20+ agents but over-engineered
  for the common case of 3–10 long-lived agents. The windowed model
  matches desktop-manager intuitions, so users don't have to learn a
  custom vocabulary.

**Setup**
```bash
./demos/windowed-ui/setup.sh
```

The script boots (or reuses) `atn-server` on :7500 and POSTs 4 local
fake agents (1 coordinator + 3 workers). Open http://localhost:7500.

**Steps**
1. Default view is **Tiled**: coordinator in the left 55%, three
   workers tiled in the right 45%. The sparkline strip at the top
   shows four equal cells with live bytes/sec sparklines.
2. Click the **Stack** button. Coordinator fills ~80% of viewport;
   the three workers collapse to the bottom dock.
3. Click any dock cell — it swaps into the primary slot.
4. Click **Carousel**. One window at center, prev/next peek at the
   edges. Press `◀` / `▶` (top-bar buttons) or `←` / `→` (keyboard)
   to cycle.
5. Back in **Tiled**, click `worker-1`'s header (accent outline
   appears). Press `m` → it minimizes to the dock. Press `1` → the
   first non-minimized window is selected. Press `M` → it maximizes.
   Press `Esc` twice (first restores max, second deselects).
6. Select a worker. Press `p` — an amber outline + 📌 appear in
   its corner. Switch to Stack / Carousel: the pinned window stays
   put while the rest rearrange around it. Press `p` again to
   unpin.
7. Click inside an xterm → the **typing to PTY** badge appears near
   that panel's Send button. Now `m` types the letter `m` into the
   terminal instead of minimizing. Press `Esc` → back to window
   management.
8. Hard-refresh the browser. Layout mode, sort, pins, per-window
   states all come back.

**Cleanup**
```bash
for id in coord worker-1 worker-2 worker-3; do
  curl -X DELETE http://localhost:7500/api/agents/$id
done
```
Or Ctrl-C the terminal running `setup.sh` if it launched the server.

**Variations**
- Switch `sort` to **Recent** — the sparkline-row order + the Tiled
  worker order now follow smoothed bytes/sec.
- Pin two windows at once (e.g. coord + a worker), then flip through
  every layout. Pins stay anchored.
- With one window maximized, press `Esc` → it restores. Press `Esc`
  again → deselects.

---

## Demo 10 — atn-cli tour

**What it shows** (atn-cli saga, steps 1–5)
- Typed HTTP client for every ATN endpoint: `agents list/state/input/
  stop/restart/reconnect/delete/wait/screenshot`, `events list/send`,
  `wiki list/get/put/delete` with ETag round-trips.
- `ATN_URL` / `--base-url` for pointing at a non-default server,
  meaningful exit codes (0 ok, 1 usage, 2 not-found, 3 http/timeout,
  4 server error), table + JSON output formats.
- Replaces the curl+jq sprawl that's been growing across the demo
  scripts. Script recipes (wait-for-all-idle, tail events, ETag-
  retry loops) in [`docs/atn-cli.md`](./atn-cli.md).

**Why it matters**
- Single tool any integration (CI, ops, other tooling) can lean on
  instead of hand-rolling shell.
- `wait` with exponential backoff + canonical state aliases is the
  right primitive for "has the agent spun up?" checks.

**Setup**
```bash
cargo build -p atn-server -p atn-cli
cargo run -p atn-server       # separate terminal; leave running
export ATN_URL=http://localhost:7500
```

**Steps**
1. Empty fleet.
   ```bash
   atn-cli agents list
   # (no agents)
   ```
2. Seed a worker via REST (CLI doesn't create agents — deliberately
   narrow scope), then wait for spawn.
   ```bash
   curl -sS -X POST -H 'Content-Type: application/json' \
     -d '{"name":"cli","role":"worker","transport":"local","working_dir":".","project":"cli","agent":"bash"}' \
     $ATN_URL/api/agents
   atn-cli agents wait cli --state any-non-starting --timeout 10
   atn-cli agents list
   ```
3. Drive input + inspect via screenshot.
   ```bash
   atn-cli agents input cli 'echo HELLO_FROM_CLI'
   atn-cli agents wait cli --state idle --timeout 5
   atn-cli agents screenshot cli --rows 20 --cols 80 | tail -5
   ```
4. Send an event + tail the log.
   ```bash
   atn-cli events send \
     --from cli --to cli --kind completion_notice \
     --summary "atn-cli demo"
   sleep 3
   atn-cli events list
   ```
5. Read + write a wiki page with optimistic concurrency.
   ```bash
   ETAG=$(atn-cli --verbose wiki get Coordination/Goals 2>&1 >/dev/null \
          | awk '/^ETag:/{print $2}')
   printf '# Goals\n\n- atn-cli demo at %s\n' "$(date -Iseconds)" \
     | atn-cli wiki put Coordination/Goals --stdin --if-match "$ETAG"
   atn-cli wiki get Coordination/Goals
   ```
6. Confirm the unhappy paths exit cleanly.
   ```bash
   atn-cli agents state ghost   ; echo "exit=$?"   # 2
   atn-cli events send --from cli --kind nope --summary x ; echo "exit=$?"  # 1
   atn-cli wiki put Coordination/Goals --stdin --if-match '"stale"' \
     <<< 'x' ; echo "exit=$?"   # 2
   ```

**Cleanup**
```bash
atn-cli agents delete cli
```

**Variations**
- Point at a remote server: `atn-cli --base-url http://other:7500 agents list`.
- `--format json` everywhere for piping into `jq` when a script
  needs a specific field.

---

## Demo 11 — Events view + wiki panel

**What it shows** (dashboard-polish saga, steps 1–4)
- Events view filter bar — text search, kind chips (OR), delivered
  radio, `K / N entries` counter. Filter state persists.
- Click-to-expand cards with full JSON, formatted timestamp + `Xm ago`
  relative, linkified `wiki_link`. Only one card open at a time;
  `Esc` collapses.
- Escalation banners gain a `jump to event ▸` button that scrolls +
  expands the matching card.
- Global wiki side-panel (right-edge drawer, 340 px) with a page
  picker; polls the open page every 5 s with `If-None-Match` and
  flashes on real changes; pauses on tab hide / panel close.
- Event row's `wiki_link` reuses the open panel instead of opening
  a new tab.

**Why it matters**
- The Events log is where inter-agent coordination is legible.
  Without filters and in-place detail, it degrades into a wall of
  small rows as soon as agents start moving.
- The wiki panel is the "reference beside the terminal" we always
  wanted — one selected page stays visible across the agents
  dashboard without swapping tabs.

**Setup**
```bash
cargo build -p atn-server -p atn-cli
cargo run -p atn-server       # leave running in another terminal
export ATN_URL=http://localhost:7500
```

**Steps**
1. Seed two agents + five events (mix of kinds, one escalation).
   ```bash
   for i in coord worker-hlasm; do
     curl -sS -X POST -H 'Content-Type: application/json' \
       -d "{\"name\":\"$i\",\"role\":\"${i%%-*}\",\"transport\":\"local\",\"working_dir\":\".\",\"project\":\"demo-11\",\"agent\":\"bash\"}" \
       $ATN_URL/api/agents > /dev/null
   done
   atn-cli events send --from worker-hlasm --to coord --kind completion_notice --summary "task X done" --wiki-link Coordination__Goals
   atn-cli events send --from worker-hlasm --to coord --kind bug_fix_request --summary "parser breaks on unicode" --issue-id ATN-42
   atn-cli events send --from coord --to worker-hlasm --kind feature_request --summary "add watchdog"
   atn-cli events send --from worker-hlasm --kind blocked_notice --summary "stuck on deploy" --priority high
   atn-cli events send --from worker-hlasm --to ghost --kind needs_info --summary "who owns the watchdog?"
   ```
2. Open the dashboard → switch to the **Events** tab. You should see
   four cards (three routed + one broadcast) plus an escalation banner
   for the `needs_info` to the non-existent `ghost`.
3. **Filter**. Type `hlasm` in the search box → narrows to the three
   cards involving `worker-hlasm`. Clear it, toggle the `blocked` chip
   → one card (`stuck on deploy`). Toggle `feature` too → two cards
   (OR within the kind category). Press **✕ reset**.
4. **Detail expand**. Click the `bug_fix_request` card → expands with
   `Issue ID ATN-42`, pretty-printed JSON, timestamp. Press **Esc** →
   collapses. Click the escalation banner's **`jump to event ▸`** →
   the needs_info card scrolls into view and expands.
5. **Wiki panel**. Click **📖 Wiki panel** in the top bar. Pick
   `Coordination__Requests` from the dropdown — body swaps.
6. **Live update**. In another terminal:
   ```bash
   ETAG=$(atn-cli --verbose wiki get Coordination__Goals 2>&1 >/dev/null \
          | awk '/^ETag:/{print $2}')
   printf '# Goals\n\n- Demo 11 edit at %s\n' "$(date -Iseconds)" \
     | atn-cli wiki put Coordination__Goals --stdin --if-match "$ETAG"
   ```
   Switch the panel's dropdown back to `Coordination__Goals` — within
   ~5 s the body re-renders and briefly flashes green.
7. **Cross-link**. Expand the `read goals` completion card (or whichever
   one has a `wiki_link`). Click its `Coordination__Goals` link — the
   panel switches pages instead of opening a new tab.

**Cleanup**
```bash
for id in coord worker-hlasm; do
  curl -sS -X DELETE $ATN_URL/api/agents/$id
done
```

**Variations**
- Close the wiki panel, open devtools → **Network** tab, wait 10 s —
  no `/api/wiki/` requests should fire (polling is paused).
- Hard-refresh the browser after setting a filter + panel open —
  both restore from `atn-window-ui-v1`.

---

## Picking one for a short slot

- **Under 5 min, no infra**: Demo 1 + the preview bit of Demo 2.
- **5–10 min, no infra**: Demo 9 (windowed UI) is the best tour of
  the whole UX in the shortest time. For the heat-sized variant,
  Demo 3 + the middle of Demo 6 still work (legacy).
- **10–15 min, real rack host**: Demos 1 → 2 (preview) → 4 (reconnect)
  → 5 (cleanup). That's the full uber-use-case story.
- **Integrations / API-minded audience**: Demo 10 (atn-cli tour) —
  one tool covers every REST endpoint with meaningful exit codes
  and `wait` for state polling. Demo 7 is still worth a quick
  detour if the audience wants to see the raw curl shapes.
- **Coordination / long-running multi-agent flow**: Demo 11
  (Events view polish + wiki side-panel) — show how the dashboard
  scans a growing event log and keeps a reference wiki page live
  alongside the agents.

## When new demos arrive

- Future Ollama / CUDA / git-sync sagas: each gets its own section here.

See also:
- [docs/usage.md](./usage.md) — operational guide
- [docs/uber-use-case.md](./uber-use-case.md) — design story
- [docs/demo-three-agent.md](./demo-three-agent.md) — the three-agent
  demo in more depth
- [docs/windowed-ui.md](./windowed-ui.md) — the windowed-UI walkthrough
  (primary model)
- [docs/events-view.md](./events-view.md) — filter/search + detail
  expand + wiki-link cross-reference
- [docs/atn-cli.md](./atn-cli.md) — typed CLI reference + recipes
- [docs/scale-ui.md](./scale-ui.md) — the 21-agent scale-UI walkthrough
  (legacy)
- [docs/remote-pty.md](./remote-pty.md) — manual remote PTY
  walkthrough
