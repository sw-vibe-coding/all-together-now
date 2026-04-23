## Phase 2: Minimal Web UI

Build one agent panel in the browser with streaming terminal output.

### Deliverables

1. Axum server with SSE endpoint for terminal bytes per agent
2. REST endpoints: POST /api/agents/{id}/input, POST /api/agents/{id}/ctrl-c
3. Yew app shell with single agent panel
4. xterm.js integration for terminal rendering in browser
5. Input box + Send button + Ctrl-C button
6. Trunk build setup for WASM

### Acceptance Criteria

- Browser at localhost:7500 shows a single agent panel
- Terminal output streams in real-time via SSE to xterm.js widget
- User can type input and send Ctrl-C from the browser
- cargo test --workspace passes
- cargo clippy --workspace -- -D warnings clean
