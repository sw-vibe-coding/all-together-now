ATN — git-sync-agents

Out-of-band PR flow from `docs/uber-use-case.md` §Topology — the
"sync agents" that watch dev-user repos for `ready-to-PR` markers,
push branches to a central remote, and open PR-equivalents using
local tooling.

## Why

Today, when an agent finishes work in its dev-user-isolated repo,
there's no mechanism to ship that work upstream short of a human
operator. The uber-use-case doc sketches a separate class of
agents (not owned by ATN's PTY) that:

  - watch dev-user repos for completion markers
  - push branches to a central remote
  - open PR-equivalents (not GitHub.com PRs — local tooling)
  - mirror merged changes back to peer r/o clones

This saga lands a fully-local subset that can run on one machine
(simulating "dev users" as sibling directories with their own git
repos + a central bare repo). The mosh / multi-host extension
follows once the on-host story works.

## Scope (in)

- A new `atn-syncd` daemon binary that watches one repo for a
  marker file, pushes the named branch to a configured remote,
  and writes a "PR" record JSON.
- A central PR registry directory (e.g. `<atn-dir>/prs/`).
- Server REST surface: `GET /api/prs`, `POST /api/prs/{id}/merge`,
  `POST /api/prs/{id}/reject`.
- atn-cli subcommands: `prs list / show / merge / reject`.
- Integration test using two local git repos + a bare central.
- Docs + Demo 13 + status rows.

## Scope (out)

- Multi-host / mosh transport. The daemon runs on the same box
  as the central repo for this saga.
- GitHub.com PRs.
- Diff rendering in the dashboard. We surface metadata + a
  `git diff` command the user can run locally.
- Mirror-back to peer r/o clones (the spec calls for this; we
  emit a merge event and document a manual fetch step here).

## Steps

1. syncd-scaffold — new `atn-syncd` crate. CLI: `--repo <path>`,
   `--remote <name>`, `--marker <filename>` (default
   `.atn-ready-to-pr`), `--prs-dir <path>`, `--poll-secs`,
   `--agent-id`, `--dry-run`. Main loop: scan repo for the marker,
   log "marker present" / "marker absent", no git actions yet.
   3–4 unit tests on path resolution + marker detection.

2. syncd-push-and-record — when the marker shows up:
   - Read its content (a YAML/JSON-ish key=value: `branch=`, `summary=`,
     `target=` — sane defaults via `git rev-parse --abbrev-ref HEAD`).
   - Run `git push <remote> <branch>:refs/heads/pr/<agent-id>-<branch>`.
   - Write a `PrRecord` JSON to `<prs-dir>/<id>.json` (id =
     `<agent-id>-<branch>-<short-sha>`).
   - Rename the marker to `.atn-ready-to-pr.queued` so it's idempotent.
   Unit tests build a real local git repo + bare remote in tempdirs
   and assert the push lands + the JSON record matches.

3. prs-rest-endpoint — atn-server `GET /api/prs` (list) +
   `GET /api/prs/{id}` (single) + `POST /api/prs/{id}/merge` +
   `POST /api/prs/{id}/reject`. Backed by the on-disk JSON files
   in `<prs-dir>`. Merge runs `git merge` on the central repo.
   Reject just updates the JSON status. Integration test boots the
   server with a seeded prs-dir + bare central + worktree, calls
   the endpoints, asserts the merge lands a commit on the central
   `main`. atn-server gains a `--prs-dir` CLI flag.

4. prs-cli — atn-cli `prs` subcommand: `list [--format json|table]`,
   `show <id>`, `merge <id>`, `reject <id>`. Mirrors the REST surface
   with consistent exit codes (0/1/2/3/4 like every other atn-cli
   subcommand). New unit tests + an addition to the existing atn-cli
   integration test that hits the new endpoints.

5. integration-and-docs — end-to-end demo: two local repos
   (`alice`, `bob`) plus a bare `central.git`, an `atn-syncd`
   running for each, a marker drop in `alice`, the daemon pushes
   the branch + writes a PR, atn-cli merges it, the central
   `main` carries the new commit. Plus `docs/git-sync-agents.md`,
   `demos/git-sync/setup.sh`, Demo 13 in demos-scripts.md,
   G1..G5 status rows + Current State flip.

## Success metrics

- One test agent drops `.atn-ready-to-pr` on a feature branch.
- `atn-syncd` notices within `--poll-secs`, pushes the branch to
  the central bare remote (`refs/heads/pr/<agent>-<branch>`), and
  writes a PR JSON.
- `atn-cli prs list` shows the PR; `atn-cli prs merge <id>` runs
  `git merge` on the central worktree and lands the commit on
  `main`.
- cargo test --workspace, clippy -D warnings, doc all clean.
