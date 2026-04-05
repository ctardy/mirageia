use crate::proxy::router::Provider;

/// Text field extracted from a JSON body, with its path for reconstruction.
#[derive(Debug, Clone)]
pub struct TextField {
    pub text: String,
    pub path: JsonPath,
}

/// JSON path to a text field.
#[derive(Debug, Clone)]
pub struct JsonPath {
    pub segments: Vec<JsonPathSegment>,
}

#[derive(Debug, Clone)]
pub enum JsonPathSegment {
    Key(String),
    Index(usize),
}

/// Extracts all text fields from a JSON body according to the provider.
pub fn extract_text_fields(body: &serde_json::Value, provider: Provider) -> Vec<TextField> {
    let mut fields = Vec::new();

    match provider {
        Provider::Anthropic => extract_anthropic_fields(body, &mut fields),
        Provider::OpenAI => extract_openai_fields(body, &mut fields),
    }

    fields
}

fn extract_anthropic_fields(body: &serde_json::Value, fields: &mut Vec<TextField>) {
    // "system" field (can be a string or an array)
    if let Some(system) = body.get("system") {
        if let Some(s) = system.as_str() {
            fields.push(TextField {
                text: s.to_string(),
                path: JsonPath {
                    segments: vec![JsonPathSegment::Key("system".to_string())],
                },
            });
        } else if let Some(arr) = system.as_array() {
            for (i, item) in arr.iter().enumerate() {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    fields.push(TextField {
                        text: text.to_string(),
                        path: JsonPath {
                            segments: vec![
                                JsonPathSegment::Key("system".to_string()),
                                JsonPathSegment::Index(i),
                                JsonPathSegment::Key("text".to_string()),
                            ],
                        },
                    });
                }
            }
        }
    }

    // "messages" field
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for (i, msg) in messages.iter().enumerate() {
            if let Some(content) = msg.get("content") {
                if let Some(s) = content.as_str() {
                    fields.push(TextField {
                        text: s.to_string(),
                        path: JsonPath {
                            segments: vec![
                                JsonPathSegment::Key("messages".to_string()),
                                JsonPathSegment::Index(i),
                                JsonPathSegment::Key("content".to_string()),
                            ],
                        },
                    });
                } else if let Some(arr) = content.as_array() {
                    for (j, block) in arr.iter().enumerate() {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                fields.push(TextField {
                                    text: text.to_string(),
                                    path: JsonPath {
                                        segments: vec![
                                            JsonPathSegment::Key("messages".to_string()),
                                            JsonPathSegment::Index(i),
                                            JsonPathSegment::Key("content".to_string()),
                                            JsonPathSegment::Index(j),
                                            JsonPathSegment::Key("text".to_string()),
                                        ],
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}

fn extract_openai_fields(body: &serde_json::Value, fields: &mut Vec<TextField>) {
    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for (i, msg) in messages.iter().enumerate() {
            if let Some(content) = msg.get("content") {
                if let Some(s) = content.as_str() {
                    fields.push(TextField {
                        text: s.to_string(),
                        path: JsonPath {
                            segments: vec![
                                JsonPathSegment::Key("messages".to_string()),
                                JsonPathSegment::Index(i),
                                JsonPathSegment::Key("content".to_string()),
                            ],
                        },
                    });
                } else if let Some(arr) = content.as_array() {
                    for (j, block) in arr.iter().enumerate() {
                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                fields.push(TextField {
                                    text: text.to_string(),
                                    path: JsonPath {
                                        segments: vec![
                                            JsonPathSegment::Key("messages".to_string()),
                                            JsonPathSegment::Index(i),
                                            JsonPathSegment::Key("content".to_string()),
                                            JsonPathSegment::Index(j),
                                            JsonPathSegment::Key("text".to_string()),
                                        ],
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Rebuilds the JSON body by replacing text fields with their pseudonymized versions.
pub fn rebuild_body(
    body: &serde_json::Value,
    replacements: &[(JsonPath, String)],
) -> serde_json::Value {
    let mut body = body.clone();

    for (path, new_text) in replacements {
        set_at_path(&mut body, &path.segments, new_text);
    }

    body
}

fn set_at_path(value: &mut serde_json::Value, segments: &[JsonPathSegment], new_text: &str) {
    if segments.is_empty() {
        *value = serde_json::Value::String(new_text.to_string());
        return;
    }

    match &segments[0] {
        JsonPathSegment::Key(key) => {
            if let Some(obj) = value.as_object_mut() {
                if let Some(child) = obj.get_mut(key) {
                    if segments.len() == 1 {
                        *child = serde_json::Value::String(new_text.to_string());
                    } else {
                        set_at_path(child, &segments[1..], new_text);
                    }
                }
            }
        }
        JsonPathSegment::Index(idx) => {
            if let Some(arr) = value.as_array_mut() {
                if let Some(child) = arr.get_mut(*idx) {
                    set_at_path(child, &segments[1..], new_text);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_anthropic_string_content() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                {"role": "user", "content": "Mon email est jean@acme.fr"}
            ]
        });

        let fields = extract_text_fields(&body, Provider::Anthropic);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].text, "Mon email est jean@acme.fr");
    }

    #[test]
    fn test_extract_anthropic_array_content() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Voici mon code"},
                    {"type": "image", "source": {"data": "base64..."}},
                    {"type": "text", "text": "IP: 192.168.1.1"}
                ]}
            ]
        });

        let fields = extract_text_fields(&body, Provider::Anthropic);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].text, "Voici mon code");
        assert_eq!(fields[1].text, "IP: 192.168.1.1");
    }

    #[test]
    fn test_extract_anthropic_system_string() {
        let body = json!({
            "system": "Tu es un assistant. Mon nom est Jean.",
            "messages": [
                {"role": "user", "content": "Bonjour"}
            ]
        });

        let fields = extract_text_fields(&body, Provider::Anthropic);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].text, "Tu es un assistant. Mon nom est Jean.");
        assert_eq!(fields[1].text, "Bonjour");
    }

    #[test]
    fn test_extract_anthropic_system_array() {
        let body = json!({
            "system": [
                {"type": "text", "text": "System prompt avec PII"}
            ],
            "messages": []
        });

        let fields = extract_text_fields(&body, Provider::Anthropic);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].text, "System prompt avec PII");
    }

    #[test]
    fn test_extract_openai_messages() {
        let body = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Tu es un assistant"},
                {"role": "user", "content": "Mon IP est 10.0.0.1"}
            ]
        });

        let fields = extract_text_fields(&body, Provider::OpenAI);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].text, "Tu es un assistant");
        assert_eq!(fields[1].text, "Mon IP est 10.0.0.1");
    }

    #[test]
    fn test_extract_empty_messages() {
        let body = json!({"model": "claude", "messages": []});
        let fields = extract_text_fields(&body, Provider::Anthropic);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_rebuild_body_string_content() {
        let body = json!({
            "model": "claude",
            "messages": [
                {"role": "user", "content": "Mon email est jean@acme.fr"}
            ]
        });

        let path = JsonPath {
            segments: vec![
                JsonPathSegment::Key("messages".to_string()),
                JsonPathSegment::Index(0),
                JsonPathSegment::Key("content".to_string()),
            ],
        };

        let rebuilt = rebuild_body(&body, &[(path, "Mon email est paul@example.com".to_string())]);

        assert_eq!(
            rebuilt["messages"][0]["content"],
            "Mon email est paul@example.com"
        );
        // The model is not touched
        assert_eq!(rebuilt["model"], "claude");
    }

    #[test]
    fn test_rebuild_body_nested_content() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "Original"},
                    {"type": "image", "source": "..."}
                ]}
            ]
        });

        let path = JsonPath {
            segments: vec![
                JsonPathSegment::Key("messages".to_string()),
                JsonPathSegment::Index(0),
                JsonPathSegment::Key("content".to_string()),
                JsonPathSegment::Index(0),
                JsonPathSegment::Key("text".to_string()),
            ],
        };

        let rebuilt = rebuild_body(&body, &[(path, "Modifié".to_string())]);

        assert_eq!(rebuilt["messages"][0]["content"][0]["text"], "Modifié");
        // The image is not touched
        assert_eq!(rebuilt["messages"][0]["content"][1]["type"], "image");
    }

    #[test]
    fn test_rebuild_body_multiple_replacements() {
        let body = json!({
            "system": "System avec PII",
            "messages": [
                {"role": "user", "content": "User avec PII"}
            ]
        });

        let replacements = vec![
            (
                JsonPath {
                    segments: vec![JsonPathSegment::Key("system".to_string())],
                },
                "System sans PII".to_string(),
            ),
            (
                JsonPath {
                    segments: vec![
                        JsonPathSegment::Key("messages".to_string()),
                        JsonPathSegment::Index(0),
                        JsonPathSegment::Key("content".to_string()),
                    ],
                },
                "User sans PII".to_string(),
            ),
        ];

        let rebuilt = rebuild_body(&body, &replacements);
        assert_eq!(rebuilt["system"], "System sans PII");
        assert_eq!(rebuilt["messages"][0]["content"], "User sans PII");
    }
}
