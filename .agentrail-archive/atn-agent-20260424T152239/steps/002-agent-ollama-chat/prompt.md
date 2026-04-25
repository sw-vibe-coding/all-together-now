## Step 2: Ollama /api/chat integration

Wire the scaffold up to Ollama's HTTP chat endpoint. No tool
execution yet — just send a prompt, print the response.

### Deliverables

1. `llm` module in `atn-agent` with:
   - `ChatMessage { role: String, content: String, tool_calls?: Vec<ToolCall> }`
   - `ChatRequest { model: String, messages: Vec<ChatMessage>,
     stream: bool, tools: Option<Vec<ToolSchema>> }`
   - `ChatResponse { message: ChatMessage, done: bool }`
   - `fn chat(base_url: &str, req: &ChatRequest) -> Result<ChatResponse, String>`
     that POSTs to `<base_url>/api/chat`, parses the response,
     returns the shaped struct. Implements a 60 s timeout and
     returns human-readable error strings.
2. Main loop integration:
   - On inbox hit, build the initial system + user messages.
     System prompt: "You are ATN agent <id>. You may call tools
     file_read / file_write / shell_exec / outbox_send /
     inbox_ack." (We wire the tool schemas in later steps; this
     step just establishes the wiring.)
   - User message: `[from <source_agent>] <summary>` plus the
     `wiki_link` text if present.
   - If `--dry-run`, print `would POST /api/chat (model=<m>)` and
     skip. Otherwise call `llm::chat` and print the
     `message.content` to stdout.
   - Silent failure handling: if the POST errors, log to stderr
     as `llm-error: <msg>` and continue polling (don't crash the
     agent).
3. Unit tests for `ChatRequest` / `ChatResponse` serde roundtrips
   (shape matches Ollama's /api/chat docs).
4. Don't try to run a real Ollama in tests — integration comes in
   step 5 with a stub server.

### Acceptance

- Pointing at a running Ollama (manual smoke) and sending a
  message makes the model's response appear in the agent's PTY
  window in ATN.
- `--dry-run` still functions: inbox message → "would POST …"
  line, no network calls.
- cargo test + clippy + doc clean.