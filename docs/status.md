# All Together Now — Status

## Current State: Phase 0 Complete, Phase 1 Next

Agentrail saga initialized with 7 steps (Phases 1-7).

## What Exists

| Item | Status |
|------|--------|
| Research & design exploration | Done — `docs/research.txt` |
| PRD | Done — `docs/prd.md` |
| Architecture | Done — `docs/architecture.md` |
| Detailed design | Done — `docs/design.md` |
| Implementation plan | Done — `docs/plan.md` |
| Usage guide | Done — `docs/usage.md` (projected) |
| CLAUDE.md + agentrail saga | Done — 7 steps defined |
| Cargo workspace (6 crates) | Done |
| `atn-core` types | Done — agent, event, inbox, error (6 tests) |
| `atn-pty` sessions | Done — session, reader, writer (compiles, no integration tests yet) |
| `atn-wiki` storage | Done — FileWikiStorage + coordination pages (8 tests) |
| `atn-trail` reader/CLI | Done — saga/step reader + CLI wrapper (4 tests) |
| `atn-server` HTTP/SSE | Placeholder — compiles and runs |
| `atn-ui` Yew frontend | Placeholder |

**Tests**: 18 passing, 0 failing. Zero clippy warnings.

## Upstream Projects

### agentrail-rs
**Location**: `~/github/sw-vibe-coding/agentrail-rs`
**Status**: Phases 0-5 complete. 13 CLI commands, 41 integration tests.
**Integration**: CLI binary in PATH, invoked by `atn-trail/src/cli.rs`.

### wiki-rs
**Location**: `~/github/sw-vibe-coding/wiki-rs`
**Status**: Production-quality. 6 backends, CAS, PATCH API, 80+ tests.
**Integration**: `wiki-common` crate used as path dependency (with `server` feature).

## Decisions Made

| Decision | Rationale |
|----------|-----------|
| Yew for primary UI | Consistent with wiki-rs; Rust-only stack |
| Library-first design | Enables future CLI, TUI, Emacs frontends |
| `portable-pty` for PTY | Cross-platform (macOS + Linux) |
| Axum for server | Consistent with wiki-rs; good SSE support |
| File-based wiki (MVP) | Simplest backend; swap later via trait |
| Serialized writer per PTY | Prevents input corruption |
| Outbox/inbox file routing | Agent writes to outbox, PGM routes to target inbox + PTY poke |
| Agentrail as CLI binary | Avoids library version conflicts; reads files directly for data |
| wiki-common as dependency | Reuse WikiPage, storage traits, parser, etag, patch |
| ATN owns its wiki impl | CAS and coordination logic in atn-wiki, not added to wiki-rs |
