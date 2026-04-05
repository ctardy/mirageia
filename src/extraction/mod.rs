/// Module d'extraction de texte depuis des blocs image/document dans les messages LLM.
pub mod docx;
pub mod pdf;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use crate::proxy::router::Provider;

/// Prétraite les blocs media (image/document) dans un body JSON LLM.
///
/// Parcourt tous les blocs dans `messages[].content[]`, détecte les blocs de type
/// "document" (PDF, DOCX) et les convertit en blocs `{"type": "text", "text": "..."}`.
///
/// Les blocs "image" sont laissés tels quels (OCR hors scope).
/// En cas d'échec d'extraction, le bloc est laissé tel quel (fail-open).
///
/// Retourne le nombre de blocs convertis avec succès.
pub fn preprocess_media_blocks(body: &mut serde_json::Value, _provider: Provider) -> usize {
    let mut converted = 0;

    let messages = match body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        Some(arr) => arr,
        None => return 0,
    };

    for message in messages.iter_mut() {
        let content = match message.get_mut("content").and_then(|c| c.as_array_mut()) {
            Some(arr) => arr,
            None => continue,
        };

        for block in content.iter_mut() {
            let block_type = block
                .get("type")
                .and_then(|t| t.as_str())
                .map(String::from);

            if block_type.as_deref() == Some("document") {
                if let Some(extracted) = try_extract_document(block) {
                    *block = serde_json::json!({
                        "type": "text",
                        "text": extracted,
                    });
                    converted += 1;
                } else {
                    tracing::warn!("Échec extraction document — bloc laissé tel quel");
                }
                // OCR image hors scope : on ignore les autres types
            }
        }
    }

    converted
}

/// Tente d'extraire le texte d'un bloc document Anthropic.
/// Structure attendue :
/// ```json
/// {
///   "type": "document",
///   "source": {
///     "type": "base64",
///     "media_type": "application/pdf",
///     "data": "JVBERi0..."
///   }
/// }
/// ```
fn try_extract_document(block: &serde_json::Value) -> Option<String> {
    let source = block.get("source")?;

    let source_type = source.get("type").and_then(|t| t.as_str())?;
    if source_type != "base64" {
        tracing::warn!("Source document non-base64 ('{}') — ignorée", source_type);
        return None;
    }

    let media_type = source
        .get("media_type")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let data_b64 = source.get("data").and_then(|d| d.as_str())?;

    let data = BASE64.decode(data_b64).ok().or_else(|| {
        tracing::warn!("Décodage base64 échoué pour un bloc document");
        None
    })?;

    let extracted = if media_type == "application/pdf" || media_type.contains("pdf") {
        let text = pdf::extract(&data)?;
        format!("[document PDF]\n{}", text)
    } else if media_type.contains("wordprocessingml")
        || media_type.contains("docx")
        || media_type == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    {
        let text = docx::extract(&data)?;
        format!("[document DOCX]\n{}", text)
    } else {
        tracing::warn!("Type MIME document non supporté : '{}' — ignoré", media_type);
        return None;
    };

    Some(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_preprocess_no_media_blocks() {
        let mut body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Bonjour, mon email est jean@acme.fr"}
                    ]
                }
            ]
        });

        let count = preprocess_media_blocks(&mut body, Provider::Anthropic);
        assert_eq!(count, 0, "Aucun bloc media à convertir");
        // Le contenu ne doit pas avoir changé
        assert_eq!(
            body["messages"][0]["content"][0]["text"],
            "Bonjour, mon email est jean@acme.fr"
        );
    }

    #[test]
    fn test_preprocess_no_messages() {
        let mut body = json!({"model": "claude-3"});
        let count = preprocess_media_blocks(&mut body, Provider::Anthropic);
        assert_eq!(count, 0);
    }

    #[test]
    fn test_preprocess_document_block_pdf() {
        // Utiliser la fonction helper du module pdf pour créer un PDF valide avec texte
        use crate::extraction::pdf::tests::build_minimal_pdf_with_text;

        let pdf_bytes = build_minimal_pdf_with_text("Contrat Jean Dupont");
        let data_b64 = BASE64.encode(&pdf_bytes);

        let mut body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "document",
                            "source": {
                                "type": "base64",
                                "media_type": "application/pdf",
                                "data": data_b64
                            }
                        }
                    ]
                }
            ]
        });

        let count = preprocess_media_blocks(&mut body, Provider::Anthropic);
        assert_eq!(count, 1, "Un bloc PDF doit être converti");

        let block = &body["messages"][0]["content"][0];
        assert_eq!(block["type"], "text", "Le bloc doit être converti en text");
        let text = block["text"].as_str().unwrap();
        assert!(
            text.contains("[document PDF]"),
            "Le texte doit commencer par [document PDF], obtenu : {}",
            text
        );
        assert!(
            text.contains("Jean Dupont"),
            "Doit contenir le texte extrait du PDF"
        );
    }

    #[test]
    fn test_preprocess_document_block_docx() {
        use crate::extraction::docx::build_test_docx;

        let docx_bytes = build_test_docx(
            Some(r#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Contrat Jean Dupont</w:t></w:r></w:p></w:body></w:document>"#),
            None,
            None,
        );
        let data_b64 = BASE64.encode(&docx_bytes);

        let mut body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "document",
                            "source": {
                                "type": "base64",
                                "media_type": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                                "data": data_b64
                            }
                        }
                    ]
                }
            ]
        });

        let count = preprocess_media_blocks(&mut body, Provider::Anthropic);
        assert_eq!(count, 1, "Un bloc DOCX doit être converti");

        let block = &body["messages"][0]["content"][0];
        assert_eq!(block["type"], "text", "Le bloc doit être converti en text");
        let text = block["text"].as_str().unwrap();
        assert!(
            text.contains("[document DOCX]"),
            "Le texte doit commencer par [document DOCX]"
        );
        assert!(text.contains("Jean Dupont"), "Doit contenir le texte extrait");
    }

    #[test]
    fn test_preprocess_unknown_type_passthrough() {
        let mut body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": "image/jpeg",
                                "data": "abc123"
                            }
                        }
                    ]
                }
            ]
        });

        let original = body.clone();
        let count = preprocess_media_blocks(&mut body, Provider::Anthropic);
        assert_eq!(count, 0, "Les blocs image ne doivent pas être convertis");
        assert_eq!(
            body["messages"][0]["content"][0]["type"],
            original["messages"][0]["content"][0]["type"]
        );
    }

    #[test]
    fn test_preprocess_invalid_base64_passthrough() {
        let mut body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "document",
                            "source": {
                                "type": "base64",
                                "media_type": "application/pdf",
                                "data": "!!! données invalides !!!"
                            }
                        }
                    ]
                }
            ]
        });

        // Ne doit pas paniquer — fail-open
        let count = preprocess_media_blocks(&mut body, Provider::Anthropic);
        assert_eq!(count, 0, "Base64 invalide → 0 conversions");
        // Le bloc doit être laissé tel quel
        assert_eq!(body["messages"][0]["content"][0]["type"], "document");
    }
}
