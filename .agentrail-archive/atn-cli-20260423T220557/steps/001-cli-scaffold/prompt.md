## Step 1: atn-cli scaffold + agents list/state

Stand up the `atn-cli` crate with the minimum plumbing: clap derive,
a sync HTTP client, base-URL resolution, JSON + table formatters,
and two read-only agents subcommands.

### Deliverables

1. New `crates/atn-cli/` workspace member with `[[bin]] name = "atn-cli"`.
2. Dependencies: clap with `derive`, serde + serde_json, chrono, and
   a sync HTTP client — prefer `ureq` (no heavy async runtime, low
   compile cost). Reuse `atn-core` for `AgentInfo`-like types if
   they're convenient; it's fine to define a lean CLI-side struct
   that deserializes the subset we care about.
3. Base URL resolution: `--base-url <url>` flag, `ATN_URL` env var,
   fallback to `http://localhost:7500`. Log the resolved URL on
   `--verbose`.
4. Subcommands implemented this step:
   - `atn-cli agents list [--format json|table]` — GET `/api/agents`,
     print. Table columns: id, name, role, state, stalled.
   - `atn-cli agents state <id> [--format json|table]` — GET
     `/api/agents/{id}/state`, print. 404 → exit 2 with a one-line
     "agent <id> not found" on stderr.
5. Exit-code convention documented in the top-level `--help`:
   0 = ok, 1 = usage error, 2 = not found, 3 = http error.
6. Minimal unit test coverage: URL resolution + table formatter.

### Acceptance

- `cargo run -p atn-cli -- agents list` against a running server
  produces a readable table.
- cargo check + clippy + doc warning-free.
- Tests pass.