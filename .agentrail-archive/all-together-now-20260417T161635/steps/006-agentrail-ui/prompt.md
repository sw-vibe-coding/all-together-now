## Phase 6: Agentrail Integration

Per-agent workflow tracking with ICRL injection visible in UI.

### Deliverables

1. UI: saga progress per agent (current step, step history)
2. Step completion detection triggers agentrail complete via CLI
3. Fresh context: inject agentrail next output into agent PTY
4. Skill distillation trigger (manual button in UI)

### Acceptance Criteria

- UI shows current saga step for each agent that has .agentrail/
- agentrail CLI invocations work correctly from atn-trail
- cargo test --workspace passes
