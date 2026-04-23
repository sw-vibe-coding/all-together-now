## Step 1: Config editor accepts a SpawnSpec

Close a known UX gap: the per-agent Config editor in the static
dashboard still uses the legacy flat fields and so silently
downgrades a dialog-created mosh agent's SpawnSpec on edit.

### Deliverables

1. atn-server:
   - `UpdateAgentBody` gains `spec: Option<SpawnSpec>`.
   - `update_agent`: when `body.spec` is present,
       - validate via `spec.validate()` (400 + missing fields on fail),
       - recompose launch_command,
       - update `state.agent_specs[id]` with the new spec,
       - update `state.agent_configs[id]` (name/role/launch_command/repo_path),
       - restart the session via the existing shutdown + spawn flow.
     When `body.spec` is None, keep the legacy flat path unchanged.
2. Static dashboard:
   - When `GET /api/agents` lists an agent with a `spec` field, render a
     structured Config editor (same fields as the New Agent dialog,
     minus `name` / `role` since those are read-only on edit; though
     keep `role` editable for flexibility).
   - Pre-fill from `agent.spec`; live preview of the composed command as
     the user types (reusing the preview formatter from the New Agent
     dialog).
   - Validation: same `missing: [...]` UX.
   - Agents without a spec (legacy TOML-loaded) continue to show the
     existing flat form.
3. `applyConfig` sends the right shape: `{ spec: {...} }` for
   spec-backed agents, `{ name, repo_path, role, launch_command }` for
   legacy.
4. Live smoke: create a mosh agent via dialog, open Config, change
   `working_dir` or `agent`, Apply, observe `GET /api/agents/{id}`
   shows the new spec + composed launch_command.

### Acceptance

- `cargo test --workspace` green; clippy --all-targets clean.
- Editing a spec-backed agent preserves the spec structure.
- Editing a legacy flat agent still works unchanged.
- The preview line in the structured Config editor matches the
  composed command the server actually runs.
