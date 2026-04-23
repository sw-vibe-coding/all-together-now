## Step 1: Shell-escape fix for coordinator commands

Fix the known issue where `(priority: High)` in a coordinator-generated
command breaks bash because parentheses aren't escaped. Audit every
site that composes a shell line from user/agent-sourced data and
funnel through a shared escape helper.

### Deliverables

1. New `shell_escape` helper in `atn-core` (or `atn-pty/src/writer.rs` if
   that's a tighter fit). Single-quote strategy: wrap the whole arg in
   single quotes; replace interior `'` with `'\''`. Covers parens,
   spaces, `<`, `>`, `$`, `"`, `;`, `&`, `|`, `\`, backticks, newlines.
2. Audit callers:
   - `atn-pty/src/writer.rs` `input_event_to_bytes` — `CoordinatorCommand`
     writes `{command}\r`. Callers that construct that command must
     quote any injected segment.
   - `canned_action_to_bytes` — `ClaudeGo`, `ReadWiki(page)`,
     `Ack(request_id)`. `page` and `request_id` come from external
     data and currently land unescaped.
   - atn-server push-event injector: anywhere a `CoordinatorCommand`
     is built from PushEvent fields (summary, priority, target).
3. Table-driven unit tests in `atn-core` for `shell_escape`:
   empty string, no specials, single quotes inside, mixed specials,
   unicode.
4. Integration test in `atn-pty/tests` that writes a command
   containing `(priority: High)` and shell metacharacters via
   `CoordinatorCommand`, runs it through a real bash PTY, and asserts
   that bash neither errors nor parses parens as a subshell.

### Acceptance

- `docs/status.md` known-issue bullet removed (the bash parens bug).
- cargo test, clippy, doc all clean.
- No regressions in the three-agent integration test.