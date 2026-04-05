use crate::proxy::router::Provider;

/// Parsed SSE event.
#[derive(Debug, Clone)]
pub struct SseEvent {
    /// Event type (e.g. "content_block_delta", "message_start").
    pub event_type: Option<String>,
    /// Raw event data.
    pub data: String,
    /// Extracted text delta (if this is an event containing text).
    pub text_delta: Option<String>,
    /// Indicates whether this is the last event in the stream.
    pub is_done: bool,
}

/// Parses a raw SSE chunk and extracts the text delta.
/// An SSE chunk has the format:
/// ```text
/// event: content_block_delta
/// data: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Bonjour"}}
/// ```
pub fn parse_sse_chunk(chunk: &str, provider: Provider) -> Option<SseEvent> {
    let mut event_type = None;
    let mut data = None;

    for line in chunk.lines() {
        if let Some(value) = line.strip_prefix("event: ") {
            event_type = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data: ") {
            data = Some(value.trim().to_string());
        }
    }

    let data = data?;

    // Check if this is the end-of-stream signal
    if data == "[DONE]" {
        return Some(SseEvent {
            event_type,
            data,
            text_delta: None,
            is_done: true,
        });
    }

    // Extract the text delta based on the provider
    let text_delta = extract_text_delta(&data, provider);

    Some(SseEvent {
        event_type,
        data,
        text_delta,
        is_done: false,
    })
}

/// Extracts the text delta from an SSE event based on the provider.
fn extract_text_delta(data: &str, provider: Provider) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(data).ok()?;

    match provider {
        Provider::Anthropic => {
            // Format: {"type":"content_block_delta","delta":{"type":"text_delta","text":"..."}}
            json.get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        }
        Provider::OpenAI => {
            // Format: {"choices":[{"delta":{"content":"..."}}]}
            json.get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        }
    }
}

/// Rebuilds an SSE chunk from a modified event.
pub fn rebuild_sse_chunk(event: &SseEvent, new_text: &str, provider: Provider) -> Option<String> {
    let mut json: serde_json::Value = serde_json::from_str(&event.data).ok()?;

    match provider {
        Provider::Anthropic => {
            if let Some(delta) = json.get_mut("delta") {
                if let Some(text) = delta.get_mut("text") {
                    *text = serde_json::Value::String(new_text.to_string());
                }
            }
        }
        Provider::OpenAI => {
            if let Some(choices) = json.get_mut("choices") {
                if let Some(choice) = choices.get_mut(0) {
                    if let Some(delta) = choice.get_mut("delta") {
                        if let Some(content) = delta.get_mut("content") {
                            *content = serde_json::Value::String(new_text.to_string());
                        }
                    }
                }
            }
        }
    }

    let mut result = String::new();
    if let Some(et) = &event.event_type {
        result.push_str(&format!("event: {}\n", et));
    }
    result.push_str(&format!("data: {}\n\n", json));
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_anthropic_text_delta() {
        let chunk = "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Bonjour\"}}";
        let event = parse_sse_chunk(chunk, Provider::Anthropic).unwrap();

        assert_eq!(event.event_type.as_deref(), Some("content_block_delta"));
        assert_eq!(event.text_delta.as_deref(), Some("Bonjour"));
        assert!(!event.is_done);
    }

    #[test]
    fn test_parse_openai_delta() {
        let chunk = "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"}}]}";
        let event = parse_sse_chunk(chunk, Provider::OpenAI).unwrap();

        assert_eq!(event.text_delta.as_deref(), Some("Hello"));
        assert!(!event.is_done);
    }

    #[test]
    fn test_parse_done_signal() {
        let chunk = "data: [DONE]";
        let event = parse_sse_chunk(chunk, Provider::Anthropic).unwrap();

        assert!(event.is_done);
        assert!(event.text_delta.is_none());
    }

    #[test]
    fn test_parse_no_text_delta() {
        let chunk = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01\"}}";
        let event = parse_sse_chunk(chunk, Provider::Anthropic).unwrap();

        assert!(event.text_delta.is_none());
        assert!(!event.is_done);
    }

    #[test]
    fn test_parse_empty_returns_none() {
        assert!(parse_sse_chunk("", Provider::Anthropic).is_none());
        assert!(parse_sse_chunk("just text", Provider::Anthropic).is_none());
    }

    #[test]
    fn test_rebuild_anthropic_chunk() {
        let event = SseEvent {
            event_type: Some("content_block_delta".to_string()),
            data: r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Michel"}}"#.to_string(),
            text_delta: Some("Michel".to_string()),
            is_done: false,
        };

        let rebuilt = rebuild_sse_chunk(&event, "Jean", Provider::Anthropic).unwrap();
        assert!(rebuilt.contains("\"text\":\"Jean\""));
        assert!(rebuilt.contains("event: content_block_delta"));
    }

    #[test]
    fn test_rebuild_openai_chunk() {
        let event = SseEvent {
            event_type: None,
            data: r#"{"choices":[{"index":0,"delta":{"content":"Michel"}}]}"#.to_string(),
            text_delta: Some("Michel".to_string()),
            is_done: false,
        };

        let rebuilt = rebuild_sse_chunk(&event, "Jean", Provider::OpenAI).unwrap();
        assert!(rebuilt.contains("\"content\":\"Jean\""));
    }
}
