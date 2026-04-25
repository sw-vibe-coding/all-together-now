# Git-Sync Agents

`atn-syncd` is a per-agent daemon that turns a `.atn-ready-to-pr`
marker file into a real git push + a `PrRecord` JSON the dashboard +
`atn-cli` can act on. It's the one-host subset of the flow described
in [`docs/uber-use-case.md`](./uber-use-case.md): every agent owns
its own worktree, an out-of-band daemon does the talking-to-git
work, and the central remote is the source of truth for review +
merge.

## Architecture (one-host subset)

```
┌──────────────────────┐
│ agent worktree       │
│  alice/              │       (1) agent writes the marker
│  ├─ feature.txt      │           when it's ready for review
│  └─ .atn-ready-to-pr │ ─────┐
└──────────────────────┘      │
                              ▼
┌──────────────────────────────────────────┐
│ atn-syncd (per agent)                    │
│  poll every --poll-secs                  │
│   ├─ parse marker (branch/target/summary)│
│   ├─ git push  central                   │  (2) push as
│   │      <branch>:refs/heads/pr/<agent>- │      pr/<agent>-<branch>
│   │      <branch>                        │
│   ├─ git rev-parse <branch>              │
│   ├─ write <prs-dir>/<id>.json           │  (3) PrRecord JSON
│   │      (status: open)                  │
│   └─ rename marker → .queued.<short>     │      idempotent
└──────────────────────────────────────────┘
                              │
                ┌─────────────┴────────────┐
                ▼                          ▼
┌──────────────────────────┐   ┌──────────────────────────┐
│ atn-server               │   │ central.git              │
│   GET /api/prs           │   │   refs/heads/main        │
│   POST /api/prs/{id}/    │   │   refs/heads/pr/         │
│        merge|reject      │   │     alice-feature        │
│  (operates on            │   │     bob-feature-z        │
│   --central-repo)        │   └──────────────────────────┘
└──────────────────────────┘
                ▲
                │  (4) human (or atn-cli) lists, picks, merges
                │
        ┌────────────────┐
        │ atn-cli prs    │
        │   list / show  │
        │   merge / reject│
        └────────────────┘
```

## Marker file format

`<repo>/<marker>` (default `.atn-ready-to-pr`) is one
`key=value` per line. `#`-prefixed lines and blank lines are
tolerated. Unknown keys are ignored.

| Key       | Default                                     | Purpose                                  |
|-----------|---------------------------------------------|------------------------------------------|
| `branch`  | `git rev-parse --abbrev-ref HEAD` in repo   | Source branch to push.                   |
| `target`  | `main`                                      | Intended merge target on central.        |
| `summary` | `<branch> ready for review`                 | Free-text summary surfaced in the UI.    |

Empty marker is fine — defaults fill in.

```
# example marker — alice/.atn-ready-to-pr
branch=feature-x
target=develop
summary=feature-x ready: adds the new outbox tool
```

## `atn-syncd` CLI reference

| Flag                | Default              | Purpose                                                   |
|---------------------|----------------------|-----------------------------------------------------------|
| `--repo <PATH>`     | *required*           | Path to the agent's git worktree.                         |
| `--agent-id <ID>`   | *required*           | Used to namespace the pushed branch (`pr/<id>-<branch>`). |
| `--remote <NAME>`   | `central`            | Git remote name on the agent's repo.                      |
| `--marker <FILE>`   | `.atn-ready-to-pr`   | Marker filename, repo-relative.                           |
| `--prs-dir <PATH>`  | `.atn/prs`           | Where `PrRecord` JSON files land.                         |
| `--poll-secs <N>`   | `3`                  | Seconds between watch ticks.                              |
| `--dry-run`         | off                  | Skip push + write; log `would handle marker`.             |
| `--exit-on-empty`   | off                  | Exit 0 after one marker-free pass (for tests).            |
| `--verbose`         | off                  | Log every poll tick.                                      |

### Exit codes

| Code | Meaning                                                          |
|------|------------------------------------------------------------------|
| `0`  | Clean exit (`--exit-on-empty` saw no marker, or SIGINT).         |
| `1`  | Usage error (bad `--repo`, malformed `--agent-id`, marker path). |
| `2`  | IO error setting up `--prs-dir`.                                 |

### Lifecycle

1. Marker present → parse body → resolve branch (default = current
   `HEAD`).
2. `git push <remote> <branch>:refs/heads/pr/<agent-id>-<branch>`.
3. `git rev-parse <branch>` → SHA.
4. Write `<prs-dir>/<id>.json` (`id = <agent>-<branch>-<short7>`,
   `status: open`).
5. Rename `<repo>/<marker>` → `<repo>/<marker>.queued.<short7>`.

All-or-nothing: any step's failure leaves the marker in place so
the next poll retries. Idempotency guard — if the queued path
already exists, the daemon logs a warning and skips the rename
rather than clobber an existing record.

## REST surface

`atn-server` exposes the registry through four routes (the
`--prs-dir` and `--central-repo` flags wire the directory + the
target repo for merges):

| Method | Path                          | Behaviour                                                                                            |
|--------|-------------------------------|------------------------------------------------------------------------------------------------------|
| GET    | `/api/prs`                    | List all records, sorted lexically. `?status=open\|merged\|rejected` filter. Bad JSON files skipped. |
| GET    | `/api/prs/{id}`               | Single record. 404 on miss.                                                                          |
| POST   | `/api/prs/{id}/merge`         | `git merge --no-ff refs/heads/pr/<agent>-<branch>` on `--central-repo`. 200 → updated record (status=`merged`, `merge_commit`, `merged_at`); 409 on conflict with `{error, stderr}`; 404 on miss. |
| POST   | `/api/prs/{id}/reject`        | Status flip only — no git side-effects. 200 → updated record (status=`rejected`, `rejected_at`); 409 if PR isn't `open`. |

Mutations are serialized through a single `Mutex<()>` and writes
go through tempfile + rename so concurrent reads never see a
partial JSON.

## `atn-cli prs` subcommands

```bash
atn-cli prs list [--status open|merged|rejected] [--format json|table]
atn-cli prs show <id> [--format json|table]
atn-cli prs merge <id>
atn-cli prs reject <id>
```

`list` prints a 5-col table (`ID  AGENT  BRANCH → TARGET  STATUS
SUMMARY`) with the summary truncated to 80 chars. `show` prints
one `key: value` line per field — lifecycle fields
(`merge_commit`, `merged_at`, `rejected_at`, `last_error`) are
emitted only when set. `merge` / `reject` echo the updated record
on success; on a 409 the server's `{error, stderr}` body is
parsed and the most useful field surfaces on stderr (exit 2,
matching the wiki ETag-mismatch convention).

## `PrRecord` shape

`atn-core::pr::PrRecord` is the on-disk + on-the-wire shape:

```json
{
  "id": "alice-feature-7d80570",
  "agent_id": "alice",
  "source_repo": "/path/to/alice/worktree",
  "branch": "feature",
  "target": "main",
  "commit": "7d8057045f89dfbc5436badcbbf1c7f1e9a72da6",
  "summary": "feature ready for review",
  "status": "open",
  "created_at": "2026-04-25T00:53:50Z",
  "merge_commit": "0011223344...",
  "merged_at": "2026-04-25T01:00:00Z"
}
```

`merge_commit` / `merged_at` / `rejected_at` / `last_error` are
optional and elided when unset. `status` is one of `open`,
`merged`, `rejected` (snake_case).

## Demo

End-to-end walkthrough with two agents + a central remote:
[`demos-scripts.md § Demo 13`](./demos-scripts.md#demo-13--git-sync-agents-end-to-end).
The script in `demos/git-sync/setup.sh` builds the tempdir layout,
spawns `atn-server` + two `atn-syncd` processes, drops markers on
both worktrees, and runs `atn-cli prs list / merge` to land both
PRs on central main.

## Known limitations

- **No GitHub PRs.** This is the *one-host subset* — the central
  remote is a local bare git directory. The same machinery can
  point at a GitHub remote, but the lifecycle endpoints
  (`merge` / `reject`) shell out to local `git merge`, not GitHub
  REST. A "full PR" mode is future work.
- **No diff in the dashboard.** `PrRecord` carries the SHA, but
  the dashboard doesn't render the diff yet. `atn-cli prs show`
  only prints metadata — pipe the SHA into `git show` for the
  diff.
- **Manual mirror-back.** If you want the agent worktrees to see
  the merged commit on `main`, do a `git pull central main` from
  each. The daemon doesn't auto-fetch.
- **Single host only.** `--central-repo` is a local path. A
  multi-host deployment (the full uber-use-case) needs ssh +
  per-host central mirrors.
- **`git merge` only.** Rebase / squash / fast-forward-only
  policies are not modelled — the merge is `--no-ff` with a fixed
  message.
