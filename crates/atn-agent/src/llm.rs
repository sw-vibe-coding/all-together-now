//! Ollama-compatible `/api/chat` client.
//!
//! Target the non-streaming shape from the Ollama docs:
//! <https://github.com/ollama/ollama/blob/main/docs/api.md#generate-a-chat-completion>.
//! opencode / lm-studio mirror the same fields, so the CLI doesn't
//! need a provider enum yet.
//!
//! This saga step (agent-ollama-chat) only sends a prompt and parses
//! back the `message.content` + optional `message.tool_calls` —
//! tool execution lands in step 3.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Client timeout for an entire `/api/chat` request. Picked long
/// enough for slow local models; ATN's watchdog handles the
/// "agent stalled" side of things.
pub const CHAT_TIMEOUT: Duration = Duration::from_secs(60);

/// One message in a chat-completion conversation.
///
/// Matches Ollama's role / content / tool_calls shape. We include
/// `tool_call_id` for the `{role: "tool"}` response turn (step 3
/// uses it). `skip_serializing_if` keeps the payload small for the
/// common "system" / "user" messages.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system<S: Into<String>>(content: S) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user<S: Into<String>>(content: S) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// The response turn that carries a tool's output back to the
    /// model. `content` is the JSON-stringified tool result; step 3
    /// drives this (hence the `allow(dead_code)` — the binary
    /// doesn't call it in step 2 but the unit test does).
    #[allow(dead_code)]
    pub fn tool<S: Into<String>, I: Into<String>>(tool_call_id: I, content: S) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Tool descriptor the client advertises to the model. Shape matches
/// Ollama's function-tool schema (`{type: "function", function: …}`).
/// Step 2 sends an empty `tools` list; step 3 fills it in.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolSchema {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionSchema,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolFunctionSchema {
    pub name: String,
    pub description: String,
    /// JSON-Schema describing the tool's argument object. Kept as a
    /// raw `Value` so the helper crate doesn't reimplement JSON Schema.
    pub parameters: Value,
}

/// One tool invocation inside `message.tool_calls`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    /// Ollama >=0.1.48 returns an id on tool calls; older versions
    /// may omit it. We fall back to synthesizing one when needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(rename = "type", default = "default_tool_call_type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

fn default_tool_call_type() -> String {
    "function".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCallFunction {
    pub name: String,
    /// Arguments as a JSON object. Some local models emit them as a
    /// stringified JSON blob — callers handle that in tool dispatch.
    pub arguments: Value,
}

/// Request body for `POST /api/chat`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolSchema>>,
}

/// Response shape for non-streaming `/api/chat`. We ignore the
/// extra bookkeeping fields (model, created_at, total_duration, …)
/// since the agent doesn't use them; serde ignores unknown fields
/// by default.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ChatResponse {
    pub message: ChatMessage,
    #[serde(default)]
    pub done: bool,
}

/// POST `<base_url>/api/chat` with `req` and parse the response.
/// Returns a human-readable error string on transport or decode
/// failure so the caller can log it to stderr without dragging in
/// `anyhow` / `thiserror`.
pub fn chat(base_url: &str, req: &ChatRequest) -> Result<ChatResponse, String> {
    let url = format!("{}/api/chat", base_url.trim_end_matches('/'));
    let resp = ureq::post(&url)
        .timeout(CHAT_TIMEOUT)
        .set("Content-Type", "application/json")
        .send_json(serde_json::to_value(req).map_err(|e| e.to_string())?)
        .map_err(|e| match e {
            ureq::Error::Status(code, r) => {
                let body = r.into_string().unwrap_or_default();
                format!("{url} returned {code}: {body}")
            }
            ureq::Error::Transport(t) => format!("transport error calling {url}: {t}"),
        })?;
    let body = resp.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str::<ChatResponse>(&body)
        .map_err(|e| format!("decode ChatResponse from {url}: {e}; body={body}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_request_serializes_minimal_shape() {
        let req = ChatRequest {
            model: "qwen3:8b".into(),
            messages: vec![
                ChatMessage::system("you are atn-agent"),
                ChatMessage::user("hello"),
            ],
            stream: false,
            tools: None,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["model"], "qwen3:8b");
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][0]["content"], "you are atn-agent");
        assert_eq!(v["messages"][1]["role"], "user");
        assert_eq!(v["stream"], false);
        // `tools` is None — skip_serializing_if keeps the payload tight.
        assert!(v.get("tools").is_none());
    }

    #[test]
    fn chat_request_includes_tools_when_some() {
        let tools = vec![ToolSchema {
            kind: "function".into(),
            function: ToolFunctionSchema {
                name: "file_read".into(),
                description: "read a file".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                }),
            },
        }];
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![],
            stream: false,
            tools: Some(tools),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["tools"][0]["type"], "function");
        assert_eq!(v["tools"][0]["function"]["name"], "file_read");
    }

    #[test]
    fn chat_response_parses_content_only() {
        let raw = r#"{
            "model": "qwen3:8b",
            "created_at": "2026-04-24T10:00:00Z",
            "message": {"role": "assistant", "content": "hi there"},
            "done": true
        }"#;
        let resp: ChatResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(resp.message.role, "assistant");
        assert_eq!(resp.message.content, "hi there");
        assert!(resp.message.tool_calls.is_none());
        assert!(resp.done);
    }

    #[test]
    fn chat_response_parses_tool_calls() {
        let raw = r#"{
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "file_read",
                            "arguments": {"path": "notes.md"}
                        }
                    }
                ]
            },
            "done": false
        }"#;
        let resp: ChatResponse = serde_json::from_str(raw).unwrap();
        let calls = resp.message.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "file_read");
        assert_eq!(calls[0].function.arguments["path"], "notes.md");
        assert_eq!(calls[0].id.as_deref(), Some("call-1"));
    }

    #[test]
    fn chat_response_tolerates_missing_tool_call_id() {
        // Some older Ollama builds omit the id. Default to None and
        // let the caller synthesize one if it needs to.
        let raw = r#"{
            "message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"function": {"name": "file_read", "arguments": {"path": "a"}}}
                ]
            }
        }"#;
        let resp: ChatResponse = serde_json::from_str(raw).unwrap();
        let calls = resp.message.tool_calls.as_ref().unwrap();
        assert!(calls[0].id.is_none());
        assert_eq!(calls[0].kind, "function"); // default_tool_call_type
    }

    #[test]
    fn tool_message_carries_tool_call_id() {
        let m = ChatMessage::tool("call-xyz", r#"{"ok": true}"#);
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["role"], "tool");
        assert_eq!(v["content"], r#"{"ok": true}"#);
        assert_eq!(v["tool_call_id"], "call-xyz");
    }

    #[test]
    fn chat_transport_error_has_url_in_message() {
        // Point at a port nothing is listening on; ureq should
        // bubble up a transport error with the URL in the message.
        let err = chat(
            "http://127.0.0.1:1",
            &ChatRequest {
                model: "m".into(),
                messages: vec![ChatMessage::user("x")],
                stream: false,
                tools: None,
            },
        )
        .unwrap_err();
        assert!(err.contains("127.0.0.1:1"), "error missing url: {err}");
    }
}
