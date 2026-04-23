ATN — Coordinator & Worker Dialog Flow

The New Agent dialog currently treats every agent the same. The real
usage pattern is asymmetric:

  Coordinator: created once, at the start. Local. Dir = a project root
               (often `.`). Agent = `claude`. That's 3 fields worth
               of input; the rest are noise.

  Workers: created many (2nd..Nth) in quick succession. Usually the
           same remote host, different users, different agent CLIs,
           different working dirs. Typing the same host on every
           worker-create is friction.

This single-step saga tightens the dialog for both paths.

Single step:

1. dialog-coordinator-workers
   - When transport=local, user/host fields are hidden (not just
     disabled). readSpawnSpec emits `user: null, host: null` so they
     never leak into the composed command.
   - Same hide-when-local treatment in the Config editor's spec mode.
   - localStorage key `atn-last-host` remembers the host after any
     successful remote-transport agent create. When the dialog opens
     next and the user picks a non-local transport, the host field is
     pre-filled from `atn-last-host` (if empty). User + dir + agent
     still need entry, matching the "usually different" observation.
   - Opening the + New Agent button focuses the `name` input so users
     can start typing immediately.
   - No default role/transport change — user still picks both each
     time; the form just gets out of their way.
