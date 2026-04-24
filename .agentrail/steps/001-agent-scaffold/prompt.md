## Step 1: atn-agent scaffold + inbox poll

Stand up the `atn-agent` crate. Clap CLI + lifecycle loop that polls
the inbox and acks messages — no LLM calls yet. This step proves
the agent looks healthy from ATN's PTY state tracker and the inbox
file convention is respected.

### Deliverables

1. New `crates/atn-agent/` workspace member with
   `[[bin]] name = "atn-agent"`. Dependencies: clap derive,
   serde + serde_json, ureq (for step 2's HTTP calls — add now so
   the dep layout is stable), `atn-core` (PushEvent + inbox consts).
2. CLI flags (mostly stubs this step, wired in later):
   - `--agent-id <id>` (required)
   - `--base-url <url>` (default `http://localhost:11434`; unused
     this step)
   - `--model <name>` (default `qwen3:8b`; unused this step)
   - `--atn-dir <path>` (default `.atn`)
   - `--workspace <path>` (default `.`)
   - `--inbox-poll-secs <N>` (default `2`)
   - `--max-tool-iterations <N>` (default `8`)
   - `--allow-shell` (flag; default false)
   - `--dry-run` (flag; default false — echoes "would call LLM"
     instead of making requests)
3. Main loop:
   - Print a banner `atn-agent: <id> up (model=<model>)` so the
     PTY state tracker flips the agent to `Running`.
   - Every `--inbox-poll-secs`, scan
     `<atn-dir>/inboxes/<agent-id>/` for `*.json`. Parse each as
     an `InboxMessage` (reuse `atn_core::inbox`). Print
     `inbox: <id> — <summary>` to stdout, then rename to
     `.json.done`.
   - SIGINT / SIGTERM: break the loop + exit 0.
4. Log-level: a `--verbose` flag that enables a short log prefix
   on each inbox tick; otherwise only print banner + inbox hits.
5. 3–4 unit tests:
   - CLI arg parsing defaults resolve as documented.
   - `InboxMessage`-ish JSON roundtrip (confirm atn-core's shape
     is what we write to stdout).
   - Path-safety for `<atn-dir>/inboxes/<agent-id>/` resolution.

### Acceptance

- `cargo run -p atn-agent -- --agent-id demo --dry-run` prints the
  banner, polls, and exits on SIGINT.
- Running under the atn-server as an agent launch_command shows
  the agent transitioning Starting → Running via the banner.
- cargo test + clippy + doc clean.