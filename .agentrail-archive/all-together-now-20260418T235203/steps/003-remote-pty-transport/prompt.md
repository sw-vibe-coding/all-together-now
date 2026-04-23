## Step 3: Remote PTY Transport

Teach the PTY subsystem to run composed `mosh`/`ssh` + `tmux` commands reliably
and recover from transport blips.

### Deliverables

1. PTY spawner runs the composed command as-is (the command already contains
   the full `mosh ... -- tmux new-session -A -s atn-<name> 'cd <dir> && <agent>'`
   pipeline from step 2). No new special casing — just ensure env, TERM, and
   window size propagate correctly to an interactive agent CLI inside tmux.
2. Fake transport binary (`tools/fake-mosh`) that records its argv and streams
   a canned script back on stdout. Used by unit tests to assert exact command
   composition without needing a real rack server.
3. Reconnect behavior: if the PTY exits (mosh drop, network blip), the agent
   is marked `disconnected` in state, not `crashed`. A "reconnect" control
   respawns the same `mosh ... tmux new-session -A -s atn-<name> ...` command,
   which re-attaches to the still-running tmux session server-side.
4. One manual integration test documented in `docs/demo-three-agent.md` that
   walks through a real queenbee mosh+tmux session (skipped in CI).
5. Graceful termination: destroying an agent from the UI sends `C-b :kill-session`
   through the PTY before closing it, so the remote tmux session is cleaned up.

### Acceptance

- Unit test asserts composed argv for mosh and ssh transports byte-for-byte.
- Simulated mosh drop (kill fake-mosh) leaves agent in `disconnected` state;
  reconnect respawns cleanly.
- Destroying an agent cleans up the remote tmux session name.
- `cargo test --workspace` green; clippy clean.
