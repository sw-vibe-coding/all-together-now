# Changes

- Standalone wiki HTML UI: browsable wiki at /wiki with sidebar navigation, edit/create pages, classic wiki-links ([[Page Name]]) with red links for missing pages that open create forms on click, "Open in Wiki" link from the ATN Wiki tab; agent list sorting (coordinator always first in all views); "Open in Wiki" link in ATN tab toolbar
- Phase 9: Interactive TUI agents + multi-saga UI + agent-to-agent delegation — PTY resize sync (browser↔PTY), TERM=xterm-256color for TUI apps, \r instead of \n for Enter in TUI mode, raw keystroke forwarding from xterm.js (arrows/Tab/Escape), raw_bytes input API, per-agent saga panels side-by-side in Saga tab, router delivers events as TUI prompts (not bash comments), outbox path fix for remote base_dir, submit_event uses base_dir
- atn-replay crate: Rust CLI for rendering PTY transcript.log files; screenshot (--html standalone, --html-fragment for org embed), at (offset), steps (chunked), dashboard (org-mode for emacs auto-revert), list, text (plain)
- Input logging: writer task logs all InputEvent to inputs.jsonl with RFC3339 timestamps for correlation with transcript.log
- Demo: 2-agent app-building scenario — dev creates a CLI app with "greet" command, ATN routes feature request to second agent who adds "farewell" command, both verified; reg-rs regression test; docs/demo-review.org with PTY screen captures; docs/needed-tools.md
- Phase 8: Session management UI — agent CRUD REST endpoints (POST/PUT/DELETE /api/agents), stop endpoint, save-to-TOML endpoint, "New Agent" form in browser, per-agent stop/delete/config-edit controls, dynamic dashboard refresh on create/destroy
- Phase 7: Polish and robustness — graceful SIGINT/SIGTERM shutdown, agent restart endpoint + UI button, dependency graph visualization (Graph tab with SVG), notification sounds via Web Audio API, agents.toml hot-reload with file watcher, panic hook + structured logging, documentation updates
- Phase 6: Agentrail integration with Saga tab UI, per-agent saga badges, step progress timeline, trajectory viewer, skill distill button, REST API (GET /api/saga, GET /api/agents/{id}/saga, POST /api/saga/distill)
- Phase 5: Message routing with outbox polling, PushEvent routing (deliver/escalate/broadcast), event log REST API, Events tab in UI
- Phase 4: Wiki integration with REST API (GET/PUT/PATCH/DELETE), ETag CAS, browser UI in both static HTML and Yew
- Phase 3: Multi-agent management with dashboard grid and state tracking
- Phase 2: Minimal web UI with SSE terminal streaming
- Phase 0+1: Workspace skeleton with PTY integration tests
