## Step 6 (inserted): Demo Scripts Documentation

Document every demo we can run against the current codebase (through
scale-UI step 5) in `docs/demos-scripts.md`. Step-by-step setup, what each
demo shows, why it matters, and how to execute it.

### Scope

Demos to cover:

1. Empty-start + New Agent dialog (local)
2. New Agent dialog with a mosh/ssh composed command preview (doesn't
   need a rack host to see the preview)
3. Three-agent topology via `demos/three-agent/setup.sh` (fake shims)
4. Reconnect after simulated mosh drop (requires a real remote host
   OR a local substitute)
5. Graceful delete clears remote tmux (requires real remote; note it
   for the audience even if we can't run it locally)
6. Treemap scale-UI: heat sizing, click-to-focus, pin row, keyboard
   shortcuts, persistence (scale-UI steps 1–5)
7. REST API tour (agents CRUD, /api/agents/heat, /api/events, /api/wiki)

Explicitly note what's NOT yet demoable: fleet of 20 fake agents
(step 8 scale-demo-docs), search/filter/groups (step 7), Ollama / CUDA
transports (separate sagas).

### Deliverables

1. `docs/demos-scripts.md` — new file structured as one section per
   demo: What it shows / Why / Setup / Steps / Cleanup / Variations.
   Each demo labeled by duration and requirements (local-only,
   needs-queenbee, etc.).
2. Cross-link from `docs/usage.md`, `docs/demo-three-agent.md`, and
   `docs/uber-use-case.md` so a reader hits it first.
3. `docs/status.md` notes the step.

### Acceptance

- A reader unfamiliar with ATN can pick any local-only demo and run it
  from the doc without reading other files.
- Commands are copy-pasteable; URLs are correct; agent names are
  consistent across demos.
- `cargo doc --workspace --no-deps` still warning-free.
