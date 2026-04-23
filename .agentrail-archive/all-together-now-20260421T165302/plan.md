ATN — Config Editor SpawnSpec Follow-On

Small single-step polish saga. The per-agent Config editor in the
static dashboard still renders the legacy flat fields (name /
repo_path / role / launch_command) and does a PUT /api/agents/{id}
with the flat UpdateAgentBody. For agents created through the New
Agent dialog (which store a structured SpawnSpec), an edit through
this form replaces launch_command with whatever the user typed and
leaves the stored spec stale — the same failure mode the step-8
spec-toml-roundtrip fix closed for Save-to-TOML.

Goal: the Config editor recognizes spec-backed agents and edits the
structured fields directly; the PUT endpoint consumes an optional
spec and re-derives launch_command from it on restart.

Single step:

1. config-editor-spawnspec
   - atn-server: UpdateAgentBody gains an optional `spec: Option<SpawnSpec>`.
     update_agent validates it, replaces state.agent_specs[id], recomposes
     launch_command, and runs the existing restart flow. When spec is None,
     keep the legacy flat behavior (backward compatible).
   - static dashboard: when an agent has `agent.spec` in the list response,
     render a structured editor (same fields as New Agent dialog) that
     pre-fills from the current spec. Fall back to the flat editor for
     agents without a spec (legacy TOML-loaded).
   - applyConfig sends the right body shape for each mode.
   - Tests + live smoke: create a mosh agent via the dialog, edit via
     Config, confirm GET /api/agents still shows the structured spec and
     the composed launch_command matches the edited fields.
