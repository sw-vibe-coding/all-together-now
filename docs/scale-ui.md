# Scale-UI Walkthrough

End-to-end tour of the scale-UI (saga steps 1–7) using a fleet of 21 fake
agents that exercise every part of the treemap. About 15 minutes if you
play through every section; 5 if you just want the money shot.

> Short on time? The quickest demo is
> [docs/demos-scripts.md § Demo 6](./demos-scripts.md#demo-6--treemap-scale-ui)
> with hand-spawned agents. This doc goes deeper using the scripted fleet.

## Prereqs

- `cargo build --workspace` has been run at least once.
- The `tools/` directory is on disk next to the repo root (ATN's
  `demos/scale/setup.sh` prepends it to `PATH` for you).
- A modern browser. No other infrastructure.

## Boot the fleet

```bash
./demos/scale/setup.sh
```

The script:

1. Launches `atn-server` on :7500 (or reuses whatever is there).
2. Waits for `/api/agents` to respond.
3. POSTs **21** agents covering five activity profiles:

   | Count | Name prefix   | Profile        | What it does                                                     |
   |------:|---------------|----------------|------------------------------------------------------------------|
   |     1 | `coord-main`  | `fake-claude`  | Startup banner then blocks on stdin — coordinator.               |
   |     4 | `spammer-0*`  | `spammer`      | Prints a line every 100 ms. Keeps heat high. Biggest tiles.      |
   |     8 | `quiet-0*`    | `quiet`        | Banner then `cat`s stdin. Tiny tiles; decay to the heat floor.   |
   |     4 | `periodic-0*` | `periodic`     | 10 s output, 30 s silent, repeat. Pulses between small and mid.  |
   |     2 | `awaiting-0*` | `awaiting-input` | Prints `(y/n)` and blocks. State boost → always in the top band. |
   |     2 | `error-0*`    | `error`        | 5 s of activity then crashes with exit 1. Goes Disconnected.     |

Open http://localhost:7500.

## What you should see

Within a couple of seconds the dashboard fills in:

- **Focus panel** (left, ~45% width): `coord-main` auto-picked because it's
  the coordinator.
- **Treemap** (right): workers tile proportional to their smoothed heat.
  - The four spammers dominate — they're emitting bytes every 100 ms.
  - The two `awaiting-0*` tiles are large even though they're quiet, because
    the `awaiting_human_input` state boost lifts their score.
  - Quiet and periodic (during their silent half) agents tile small.
  - The two error agents start normal, then flip to **Disconnected** after
    ~5 s and their tiles shrink to the heat floor.
- **Filter bar** above the pin row is visible (first agent triggered it).
- **Pin row** is empty.

Give the fleet 30 s to settle before pressing buttons.

## Guided exercises

### 1. Click-to-focus + 5-second freeze

- Click any `spammer-0*` tile's header.
- It swaps into the focus panel; `coord-main` drops back into the treemap.
- The layout **freezes for 5 seconds** so you can actually read the
  spammer's output. After the freeze, the next poll redraws.
- Press **Esc**. Auto-pick takes over and `coord-main` returns to focus.

### 2. Pin row

- With `spammer-01` focused, press **p**.
- The agent moves into the pin row at the top with a 📌 icon. Focus
  auto-picks again (coord-main).
- Pinned agents are **excluded from the treemap** — pin sizes are stable.
- Click the × on the pinned cell to unpin (or press **p** again with the
  agent focused via re-clicking its pin).

### 3. Hottest-N keyboard shortcuts

- Press **1** — focuses the hottest unpinned tile (a spammer).
- Press **2**, **3** — second and third hottest.
- Press **0** — focuses the coordinator (`coord-main`).
- Press **f** — toggles focus panel width medium (40–50%) ↔ large (55–70%).

### 4. Filter

- Press **/** — focus jumps to the filter input.
- Type `spam` + Enter. The treemap narrows to just the 4 spammers.
  Coordinator + other workers are hidden (DOM moved off-screen).
- Clear the input. Everything is back.
- Click the **awaiting-input** chip. Only the two `awaiting-0*` agents
  remain. Notice `awaiting-0*` tiles are always hot because the state
  boost raises their score even with zero bytes/sec.
- Toggle the chip off.

### 5. Pins + filter interact

- Pin `coord-main` (focus it via **0**, then **p**).
- Type `quiet` into the filter. Pin cell still shows `coord-main` in the
  pin row but with a muted `(filtered)` suffix — pinned agents stay
  visible regardless of filter, but they're marked so you know they'd
  otherwise be hidden.
- Clear the filter; unpin (click ×).

### 6. Group by role / project

- Switch **group-by** to `role`. The treemap splits into two clusters
  (coordinator + worker), each with its own dashed band labeled with
  the role name. Tiles inside each band are still sized by heat.
- Switch to `project`. Everyone's project is `scale-demo` so you get one
  cluster — boring, but it confirms the grouping works.
- Switch back to `none`.

### 7. Saved layouts

- Build a layout you like: `/quiet` in the filter, `group-by: role`, pin
  two spammers.
- Click **Save** in the Layouts dropdown area, name it `quiet-focus`.
- Click **Apply** on a different preset (none selected yet, so select
  `quiet-focus` and Apply).
- **Refresh the browser.** Pins, focus, focus size, filter text, chip
  selections, and group-by mode all come back from localStorage. The
  Layouts dropdown still has `quiet-focus` available.
- **Delete** removes a saved layout.

### 8. Error / disconnect visibility

- The two `error-0*` agents exited with status 1 after ~5 s. Their
  state badges read **Disconnected** (step-3 exit detection). Their
  tiles are small because the `disconnected` state boost is negative,
  but the heat floor keeps them visible.
- Select the **disconnected** chip. Only the error agents remain.
- Press **r** on an error tile (or click its Restart button) to respawn
  — it runs through the 5 s cycle again and crashes again.

## Performance notes

- Tile resize is **pure CSS** — xterm never sees a resize event, even when
  the treemap is redrawn 40+ tiles / second of smoothing. Open devtools
  → Network and watch: `/api/agents/heat` fires once per second, and
  no `/api/agents/*/resize` POSTs fire unless you explicitly focus a
  tile (focus triggers a single `fitAddon.fit()` + PTY resize for the
  chosen tile only).
- Layout updates suspend for 5 s after any click-to-focus so the user
  isn't fighting the refresh.
- Heat smoothing (α ≈ 0.3 at 1 Hz, ~5 s half-life) keeps tile areas from
  jittering on small byte bursts.

## Teardown

Stop the server with Ctrl-C in the terminal where `setup.sh` is
running. Or, from another shell:

```bash
for id in coord-main spammer-0{1,2,3,4} quiet-0{1,2,3,4,5,6,7,8} \
          periodic-0{1,2,3,4} awaiting-0{1,2} error-0{1,2}; do
    curl -X DELETE http://localhost:7500/api/agents/$id
done
```

## See also

- [docs/demos-scripts.md](./demos-scripts.md) — full menu of demos
- [docs/usage.md](./usage.md) — REST API, env vars, New Agent dialog
- [docs/uber-use-case.md](./uber-use-case.md) — isolation-first design
