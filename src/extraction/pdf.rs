/// Text extraction from a PDF file via lopdf.
use lopdf::Document;

/// Extracts text from a PDF provided as bytes.
/// Returns None if the data is invalid or no text can be extracted.
pub fn extract(data: &[u8]) -> Option<String> {
    let doc = Document::load_mem(data).ok()?;

    let mut parts: Vec<String> = Vec::new();

    // Metadata via the trailer -> Info dictionary
    if let Ok(info_id) = doc.trailer.get(b"Info").and_then(lopdf::Object::as_reference) {
        if let Ok(info_dict) = doc.get_dictionary(info_id) {
            let mut meta_lines: Vec<String> = Vec::new();

            for (key, field_name) in &[
                (b"Author" as &[u8], "auteur"),
                (b"Company", "société"),
                (b"Title", "titre"),
                (b"Subject", "sujet"),
            ] {
                if let Ok(obj) = info_dict.get(key) {
                    if let Ok(s) = obj.as_string() {
                        let s = s.trim();
                        if !s.is_empty() {
                            meta_lines.push(format!("{}: {}", field_name, s));
                        }
                    }
                }
            }

            if !meta_lines.is_empty() {
                parts.push(format!("[métadonnées]\n{}", meta_lines.join("\n")));
            }
        }
    }

    // Pages extraction
    let mut page_numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    page_numbers.sort_unstable();

    for page_num in page_numbers {
        match doc.extract_text(&[page_num]) {
            Ok(text) if !text.trim().is_empty() => {
                parts.push(format!("[page {}]\n{}", page_num, text.trim()));
            }
            _ => {}
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(parts.join("\n\n"))
}

/// Test module exposing reusable helpers for tests in other modules.
#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_extract_pdf_empty_bytes() {
        assert!(extract(&[]).is_none());
    }

    #[test]
    fn test_extract_pdf_invalid_bytes() {
        assert!(extract(b"not a pdf at all").is_none());
    }

    /// Builds a minimal valid PDF with extractable text via the lopdf API.
    pub fn build_minimal_pdf_with_text(text_content: &str) -> Vec<u8> {
        use lopdf::dictionary;
        use lopdf::content::{Content, Operation};
        use lopdf::{Document, Object, Stream};

        let mut doc = Document::with_version("1.5");

        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Courier",
        });

        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! {
                "F1" => font_id,
            },
        });

        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 48.into()]),
                Operation::new("Td", vec![100.into(), 600.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::string_literal(text_content)],
                ),
                Operation::new("ET", vec![]),
            ],
        };

        let pages_id = doc.new_object_id();
        let content_id =
            doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
        });

        let pages = dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        };
        doc.objects.insert(pages_id, Object::Dictionary(pages));

        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        buf
    }

    #[test]
    fn test_extract_pdf_text() {
        let pdf_bytes = build_minimal_pdf_with_text("Bonjour");

        let result = extract(&pdf_bytes);
        assert!(
            result.is_some(),
            "L'extraction devrait retourner du texte depuis un PDF valide"
        );
        let text = result.unwrap();
        assert!(
            text.contains("[page"),
            "Devrait contenir une section [page], obtenu : {}",
            text
        );
        assert!(
            text.contains("Bonjour"),
            "Devrait contenir le texte 'Bonjour', obtenu : {}",
            text
        );
    }
}
