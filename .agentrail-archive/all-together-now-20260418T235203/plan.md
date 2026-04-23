All Together Now — Remote-Agent Demo Saga

Goal: ATN can boot with zero agents and let a human compose any number of local or
remote agents through a "New Agent" dialog. Validate by running a concrete demo
with a local coordinator on the Mac plus two remote workers on rack server
`queenbee`, each a different agent CLI in a different repo.

Demo topology:
  - coordinator : mighty-mike (local Mac)  / atn-demo dir      / claude
  - worker-hlasm: queenbee     / devh1     / hlasm repo        / codex
  - worker-rpg  : queenbee     / devr1     / rpg-ii repo       / opencode-z-ai-glm-5

Steps:

1. empty-start
   ATN server boots cleanly with zero configured agents. UI renders an empty state
   with a single "New Agent" call-to-action. Remove seed agents from agents.toml
   (or support an empty [[agent]] list). Tests cover both empty and populated
   startup paths.

2. new-agent-dialog
   Replace the free-form command field with a structured dialog that captures:
   name, role, transport (local | mosh | ssh), host, user, working-dir,
   project/repo, agent command (claude | codex | opencode ... | custom).
   The server composes the actual shell command from these parts and stores the
   structured config alongside the derived command. Dialog posts to
   `POST /api/agents`. Form validates required fields per transport.

3. remote-pty-transport
   Teach the PTY spawner to run a composed `mosh <user>@<host> -- tmux
   new-session -A -s atn-<agent> 'cd <dir> && <agent-cli>'` command.
   Ensure stdin/stdout flow survives mosh reconnects (tmux keeps the session on
   the server). Unit tests use a fake transport binary to assert command
   composition; one manual integration test with real mosh+tmux.

4. three-agent-demo
   Ship fixtures, a demo script, and an integration test that stand up the
   full three-agent topology from an empty start by POSTing to `/api/agents`
   three times. Use fake-agent CLIs (echo loops) in CI; document how to swap
   in real claude/codex/opencode for live demos. Verify event routing and
   wiki participation across the three agents end-to-end.

5. docs-refresh
   Update docs/usage.md with the new-agent dialog flow. Add a new
   docs/demo-three-agent.md walkthrough that matches the saga demo. Cross-link
   from docs/uber-use-case.md. Refresh docs/status.md.
