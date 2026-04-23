## Step 1: Dialog flows for coordinator + workers

### Deliverables

1. Static HTML New Agent dialog: hide the user/@/host sub-group when
   `transport=local`; restore when user flips to `mosh`/`ssh`. Applies
   on dialog open and on every transport change.
2. Same treatment in the per-agent Config editor's spec-mode form
   (`cfg-spec-remote-${id}` block).
3. `readSpawnSpec` and `readSpecFromEditor` emit
   `user: null, host: null` whenever `transport === 'local'`, even if
   the hidden inputs happen to hold stale values from a prior render.
4. localStorage key `atn-last-host` remembers the host after any
   successful POST /api/agents whose transport was not `local`. When
   the dialog next opens and the user selects a non-local transport,
   the host field is pre-filled from this value if the input is empty.
5. Clicking the + New Agent button focuses the `name` input so the
   demo flow is keyboard-first.
6. No behavior change when transport is already local — existing
   three-agent demo and scale demo still work unchanged.

### Acceptance

- Create a coordinator (role=coordinator, transport=local, dir=.,
  agent=claude) without seeing or filling host/user.
- Create a worker (transport=mosh, user=devh1, host=queenbee, …) and
  then open the dialog again — host is pre-filled; user/dir/agent
  still blank.
- `cargo test --workspace` green; clippy --all-targets clean.
