## Step 3: file_read + file_write tools + tool-call loop

Implement the filesystem tools and the call-dispatch loop that
feeds results back into the chat until the model signals done.

### Deliverables

1. `tools` module defining:
   - `ToolSchema` (matches Ollama's function-tool schema —
     `{type: "function", function: {name, description, parameters}}`).
   - `ToolCall` (incoming from the model — `{function: {name, arguments: Value}}`).
   - Tool registry with two initial entries: `file_read`, `file_write`.
   - `fn dispatch(call: &ToolCall, ctx: &ToolCtx) -> ToolResult`
     where `ToolCtx` carries the workspace root + allow_shell flag
     (shell stays disabled here).
2. Path sandboxing helper `sandbox_path(workspace, user_path)`:
   - Rejects absolute paths.
   - Rejects paths that normalize outside the workspace (`..`).
   - Returns the canonical path + a friendly error message.
   - Table-driven tests covering the common bypass attempts.
3. `file_read(path)`:
   - Sandbox the path, read up to 256 KiB, return contents as a
     string. Larger files return a truncation notice so the model
     knows the content isn't complete.
4. `file_write(path, content)`:
   - Sandbox the path, create parent dirs, write contents. Enforce
     a 1 MiB write ceiling. Return `"wrote N bytes to <path>"` on
     success.
5. Chat loop update:
   - After `llm::chat`, if `message.tool_calls` is non-empty,
     dispatch each call, append a `{role: "tool", content: …}`
     message with the JSON result, re-POST. Bail out when
     `tool_calls` is empty or after `--max-tool-iterations`
     iterations (log a warning).
   - Wire the two tool schemas into the `ChatRequest.tools` field
     for the initial call and every follow-up.
6. Unit tests:
   - `sandbox_path` accepts valid children, rejects `..` / absolute.
   - `file_read` + `file_write` round-trip happy-path (tempfile).
   - `dispatch` returns a helpful error for unknown tool names.

### Acceptance

- A manual Ollama run where the model calls `file_read("notes.md")`
  prints a tool-call log line + response; the follow-up model
  turn uses the file's contents.
- Trying to read `/etc/passwd` from inside the workspace returns
  an error (sandboxed away).
- cargo test + clippy + doc clean.