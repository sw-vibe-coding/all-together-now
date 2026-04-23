## Phase 5: Message Routing

Structured inter-agent communication through the PGM.

### Deliverables

1. Outbox/inbox directory watching (polling or notify crate)
2. PushEvent file detection and parsing from agent outboxes
3. MessageRouter: known target -> PTY inject; unknown -> wiki + broadcast
4. Human escalation for unroutable events (UI notification)
5. Push event log (append-only, viewable in UI)
6. "Awaiting input" detection heuristics for Claude Code output

### Acceptance Criteria

- Agent A writes JSON to outbox, PGM routes to Agent B inbox and pokes PTY
- Unknown-target events appear in wiki Requests page
- Event log viewable in UI
- cargo test --workspace passes
