/// Extraction de texte depuis un fichier DOCX (ZIP contenant des XML).
use std::io::{Cursor, Read};
use zip::ZipArchive;

/// Extrait le texte d'un DOCX fourni sous forme de bytes.
/// Retourne None si les données sont invalides ou si aucun texte ne peut être extrait.
pub fn extract(data: &[u8]) -> Option<String> {
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).ok()?;

    let mut parts: Vec<String> = Vec::new();

    // Contenu principal — word/document.xml
    if let Some(content_text) = read_entry_text(&mut archive, "word/document.xml") {
        let text = extract_w_t_text(&content_text);
        if !text.trim().is_empty() {
            parts.push(format!("[contenu]\n{}", text.trim()));
        }
    }

    // Commentaires — word/comments.xml (optionnel)
    if let Some(comments_text) = read_entry_text(&mut archive, "word/comments.xml") {
        let text = extract_w_t_text(&comments_text);
        if !text.trim().is_empty() {
            parts.push(format!("[commentaires]\n{}", text.trim()));
        }
    }

    // Métadonnées — docProps/core.xml (optionnel)
    if let Some(core_text) = read_entry_text(&mut archive, "docProps/core.xml") {
        let mut meta_lines: Vec<String> = Vec::new();

        if let Some(creator) = extract_xml_element(&core_text, "dc:creator") {
            if !creator.trim().is_empty() {
                meta_lines.push(format!("auteur: {}", creator.trim()));
            }
        }
        if let Some(company) = extract_xml_element(&core_text, "cp:company") {
            if !company.trim().is_empty() {
                meta_lines.push(format!("société: {}", company.trim()));
            }
        }
        if let Some(title) = extract_xml_element(&core_text, "dc:title") {
            if !title.trim().is_empty() {
                meta_lines.push(format!("titre: {}", title.trim()));
            }
        }
        if let Some(subject) = extract_xml_element(&core_text, "dc:subject") {
            if !subject.trim().is_empty() {
                meta_lines.push(format!("sujet: {}", subject.trim()));
            }
        }

        if !meta_lines.is_empty() {
            parts.push(format!("[métadonnées]\n{}", meta_lines.join("\n")));
        }
    }

    if parts.is_empty() {
        return None;
    }

    Some(parts.join("\n\n"))
}

/// Lit une entrée ZIP et retourne son contenu sous forme de String UTF-8.
fn read_entry_text(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    let mut content = String::new();
    entry.read_to_string(&mut content).ok()?;
    Some(content)
}

/// Extrait le texte des balises <w:t> dans un XML DOCX.
/// Concatène avec des espaces entre les runs et des sauts de ligne entre les paragraphes.
fn extract_w_t_text(xml: &str) -> String {
    let mut result = String::new();
    let mut current_para = String::new();
    let mut pos = 0;
    let bytes = xml.as_bytes();
    let len = bytes.len();

    while pos < len {
        // Chercher la prochaine balise ouvrante
        if let Some(tag_start) = find_bytes(bytes, pos, b"<") {
            // Lire le nom de la balise
            let tag_name_start = tag_start + 1;
            if tag_name_start >= len {
                break;
            }

            // Fermeture de paragraphe
            if (bytes[tag_name_start..].starts_with(b"/w:p>")
                || bytes[tag_name_start..].starts_with(b"w:p ")
                || bytes[tag_name_start..].starts_with(b"w:p>"))
                && bytes[tag_name_start] == b'w'
                && tag_name_start + 1 < len
                && bytes[tag_name_start + 1] == b':'
                && tag_name_start + 2 < len
                && bytes[tag_name_start + 2] == b'p'
                && bytes[tag_name_start..].starts_with(b"/w:p>")
            {
                // Fin de paragraphe
                if !current_para.trim().is_empty() {
                    if !result.is_empty() {
                        result.push('\n');
                    }
                    result.push_str(current_para.trim());
                }
                current_para.clear();
            }

            // Balise <w:t> ou <w:t xml:space="preserve">
            if bytes[tag_name_start..].starts_with(b"w:t") {
                let after_wt = tag_name_start + 3;
                let next = bytes.get(after_wt).copied();
                if next == Some(b'>') || next == Some(b' ') || next == Some(b'\n') {
                    // Trouver la fin de la balise ouvrante >
                    if let Some(end_open) = find_bytes(bytes, tag_name_start, b">") {
                        let content_start = end_open + 1;
                        // Trouver </w:t>
                        if let Some(close_start) = find_bytes_slice(bytes, content_start, b"</w:t>") {
                            let text_bytes = &bytes[content_start..close_start];
                            if let Ok(text) = std::str::from_utf8(text_bytes) {
                                // Décoder les entités XML basiques
                                let decoded = decode_xml_entities(text);
                                if !decoded.is_empty() {
                                    if !current_para.is_empty() {
                                        current_para.push(' ');
                                    }
                                    current_para.push_str(&decoded);
                                }
                            }
                            pos = close_start + 6; // longueur de </w:t>
                            continue;
                        }
                    }
                }
            }

            // Avancer après la balise
            if let Some(tag_end) = find_bytes(bytes, tag_start + 1, b">") {
                pos = tag_end + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // Ajouter le dernier paragraphe s'il n'est pas terminé par </w:p>
    if !current_para.trim().is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(current_para.trim());
    }

    result
}

/// Extrait le contenu texte d'un élément XML par son nom de balise.
fn extract_xml_element(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);

    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;

    Some(decode_xml_entities(&xml[start..end]))
}

/// Décode les entités XML basiques.
fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

/// Trouve l'index du prochain octet `needle` dans `bytes` à partir de `from`.
fn find_bytes(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.len() == 1 {
        bytes[from..].iter().position(|&b| b == needle[0]).map(|i| i + from)
    } else {
        find_bytes_slice(bytes, from, needle)
    }
}

/// Trouve l'index de la prochaine occurrence de `needle` dans `bytes` à partir de `from`.
fn find_bytes_slice(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from + needle.len() > bytes.len() {
        return None;
    }
    bytes[from..bytes.len() - needle.len() + 1]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// Construit un DOCX minimal en mémoire (pour les tests).
#[cfg(test)]
pub fn build_test_docx(
    document_xml: Option<&str>,
    comments_xml: Option<&str>,
    core_xml: Option<&str>,
) -> Vec<u8> {
    use std::io::Write;
    use zip::write::{FileOptions, ZipWriter};
    use zip::CompressionMethod;

    let buf = Vec::new();
    let cursor = Cursor::new(buf);
    let mut zip = ZipWriter::new(cursor);
    let options: FileOptions<'_, ()> = FileOptions::default()
        .compression_method(CompressionMethod::Deflated);

    if let Some(xml) = document_xml {
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    if let Some(xml) = comments_xml {
        zip.start_file("word/comments.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    if let Some(xml) = core_xml {
        zip.start_file("docProps/core.xml", options).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }

    zip.finish().unwrap().into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOCUMENT_XML_SIMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p>
      <w:r><w:t>Contrat entre Jean Dupont</w:t></w:r>
    </w:p>
    <w:p>
      <w:r><w:t>et la société ACME.</w:t></w:r>
    </w:p>
  </w:body>
</w:document>"#;

    const COMMENTS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:comment>
    <w:p>
      <w:r><w:t>À revoir avec le client</w:t></w:r>
    </w:p>
  </w:comment>
</w:comments>"#;

    const CORE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"
                   xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:creator>Jean Dupont</dc:creator>
  <dc:title>Contrat de service</dc:title>
</cp:coreProperties>"#;

    #[test]
    fn test_extract_docx_simple() {
        let docx = build_test_docx(Some(DOCUMENT_XML_SIMPLE), None, None);
        let result = extract(&docx);
        assert!(result.is_some(), "Devrait extraire du texte");
        let text = result.unwrap();
        assert!(text.contains("[contenu]"), "Devrait contenir [contenu]");
        assert!(text.contains("Jean Dupont"), "Devrait contenir le texte du document");
    }

    #[test]
    fn test_extract_docx_with_comments() {
        let docx = build_test_docx(Some(DOCUMENT_XML_SIMPLE), Some(COMMENTS_XML), None);
        let result = extract(&docx);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("[commentaires]"), "Devrait contenir [commentaires]");
        assert!(text.contains("À revoir"), "Devrait contenir le texte du commentaire");
    }

    #[test]
    fn test_extract_docx_with_metadata() {
        let docx = build_test_docx(Some(DOCUMENT_XML_SIMPLE), None, Some(CORE_XML));
        let result = extract(&docx);
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("[métadonnées]"), "Devrait contenir [métadonnées]");
        assert!(text.contains("auteur: Jean Dupont"), "Devrait contenir l'auteur");
    }

    #[test]
    fn test_extract_docx_invalid() {
        assert!(extract(b"not a docx").is_none());
        assert!(extract(b"").is_none());
    }
}
