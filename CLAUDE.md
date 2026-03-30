# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## CRITICAL: AgentRail Session Protocol (MUST follow exactly)

This project uses AgentRail. Every session follows this exact sequence:

### 1. START (do this FIRST, before anything else)
```bash
agentrail next
```
Read the output carefully. It tells you your current step, prompt, skill docs, and past trajectories.

### 2. BEGIN (immediately after reading the next output)
```bash
agentrail begin
```

### 3. WORK (do what the step prompt says)
Do NOT ask the user "want me to proceed?" or "shall I start?". The step prompt IS your instruction. Execute it.

### 4. COMMIT (after the work is done)
Commit your code changes with git.

### 5. COMPLETE (LAST thing, after committing)
```bash
agentrail complete --summary "what you accomplished" \
  --reward 1 \
  --actions "tools and approach used"
```
If the step failed: `--reward -1 --failure-mode "what went wrong"`
If the saga is finished: add `--done`

### 6. STOP (after complete, DO NOT continue working)
Do NOT make any further code changes after running agentrail complete.
Any changes after complete are untracked and invisible to the next session.
If you see more work to do, it belongs in the NEXT step, not this session.

Do NOT skip any of these steps. The next session depends on your trajectory recording.

## Project

All Together Now (ATN): a Program Manager (PGM) that orchestrates multiple AI agent sessions via PTY ownership. Provides a web UI (Yew/WASM) for human-in-the-loop oversight, inter-agent message routing, shared coordination wiki, and per-agent workflow tracking.

## Related Projects

- `~/github/sw-vibe-coding/wiki-rs` -- Wiki system (Yew + Axum); wiki-common crate used as dependency
- `~/github/sw-vibe-coding/agentrail-rs` -- Workflow CLI; used as external binary (not library)
- `~/github/sw-vibe-coding/agentrail-domain-coding` -- Coding skills domain

## Available Task Types

`rust-project-init`, `rust-workspace-setup`, `rust-pty-integration`, `yew-component`, `axum-sse-server`, `pre-commit`

## Architecture

Cargo workspace with 6 crates:
- `atn-core` -- Domain types, traits, errors (no async, no I/O)
- `atn-pty` -- PTY session management (portable-pty + tokio)
- `atn-server` -- Axum HTTP/SSE binary
- `atn-ui` -- Yew web UI components
- `atn-wiki` -- Wiki storage + coordination (depends on wiki-common)
- `atn-trail` -- Agentrail file reader + CLI wrapper

Library-first: atn-core, atn-pty, atn-wiki, atn-trail are pure libraries.
Yew is one UI frontend; CLI/TUI/Emacs are future add-ons.

## Build

```bash
cargo check --workspace        # type check
cargo clippy --workspace -- -D warnings  # lint
cargo test --workspace         # run all tests
cargo run -p atn-server        # run the PGM server
```

## Key Conventions

- Rust 2024 edition
- Zero clippy warnings (`-D warnings`)
- Two planes: terminal (raw PTY bytes) and orchestration (structured JSON events)
- Serialized writer per agent PTY (no interleaved input)
- File-based outbox/inbox for inter-agent push events
- wiki-common dependency at `../wiki-rs/shared/wiki-common` (with `server` feature)
- agentrail invoked as CLI binary, not library
