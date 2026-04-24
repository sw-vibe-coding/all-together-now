## Step 3: /api/agents/{id}/screenshot HTTP endpoint

Expose the terminal snapshot over HTTP so the UI and atn-cli can
fetch a rendered snapshot without reaching into transcript files.

### Deliverables

1. `GET /api/agents/{id}/screenshot` with query params:
   - `format=text|ansi|html` (default `text`)
   - `rows=N` (default `40`)
   - `cols=N` (default `120`)
2. Backing source: read the last ~rows*cols*8 bytes from the active
   transcript (`{log_dir}/{agent_id}/transcript.log`) or, if present,
   subscribe briefly to the live broadcast. Tail-reading is fine for
   step 3; live-tap is optional.
3. Content-Type follows `format`: `text/plain; charset=utf-8`,
   `text/plain; charset=utf-8` (ansi — document that it's raw), or
   `text/html; charset=utf-8`.
4. Integration test in `crates/atn-server/tests/` that spawns the
   three-agent demo topology, types known input into one agent, and
   asserts the screenshot text includes the expected line.

### Acceptance

- curl -s /api/agents/<id>/screenshot returns useful text for every
  demo agent.
- Bad format → 400 with a short error.
- Unknown id → 404.
- cargo test + clippy clean.