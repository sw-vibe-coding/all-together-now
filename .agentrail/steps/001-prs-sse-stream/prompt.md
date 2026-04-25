## Step 1: `/api/prs/stream` SSE endpoint + filesystem watcher

Drive the dashboard PR panel without polling. Use the `notify`
crate (already in workspace deps) to watch `<prs-dir>` for create /
modify / remove events, fan them out as SSE deltas to every
connected client.

### Deliverables

1. New module `crates/atn-server/src/prs_stream.rs`:
   - `PrsBroadcast { sender: tokio::sync::broadcast::Sender<PrsEvent> }`
     wired into `SharedState` (sibling of `PrsState`).
   - `PrsEvent` enum: `Snapshot { records }`, `Created { record }`,
     `Updated { record }`, `Removed { id }`. Snake-case JSON tag
     so the client can `match` on `event` directly.
   - `spawn_watcher(prs_dir, sender)` runs a `tokio::spawn`-ed
     task that wraps `notify::recommended_watcher` (or the
     `RecommendedWatcher::new` API) and translates filesystem
     events into `PrsEvent`s. Coalesce within ~50 ms so a
     write-to-tempfile + rename doesn't fire two events.
2. `GET /api/prs/stream` route returns
   `Sse<...>` keep-alive=15s. First message is
   `Snapshot { records }` from a one-shot `read_records`. Then
   forward each broadcast event verbatim. On client drop, the
   receiver is dropped and the broadcaster reclaims the slot.
3. atn-server boot: spawn the watcher once, before route
   registration. Log `prs watcher: watching <prs-dir>` at info.
4. Mutating routes (`merge` / `reject`) push the updated record
   onto the broadcast after writing — that way clients see a
   `Updated` event even if `notify` missed the rename (some
   filesystems coalesce overwrite events). The duplicate is
   harmless; the client de-dups by id+status.
5. Unit tests for `PrsEvent` serde (Snapshot / Created / Updated /
   Removed all serialize with the right `event` discriminant) +
   coalescing helper. Integration test extends
   `prs_endpoints.rs` with one new `#[test] fn prs_stream_pushes_a_create()`
   that opens the SSE stream via `reqwest`-style raw TCP read,
   drops a PR JSON into `<prs-dir>`, and asserts a `Created`
   event arrives within ~3 s.

### Acceptance

- `curl -N http://localhost:7500/api/prs/stream` prints a
  `Snapshot` first, then live `Created` / `Updated` /
  `Removed` lines as the prs-dir changes.
- cargo test workspace + clippy --all-targets -D warnings + doc
  clean.
