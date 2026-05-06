use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::text::detect_image_mime;

pub const MAX_TEXT_LENGTH: usize = 200_000;
pub const MAX_EXTRACT_FILE_SIZE: u64 = 50 * 1024 * 1024;

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "pdf", "docx", "xlsx", "pptx", "txt", "md", "csv", "json", "xml", "html", "htm", "log", "yaml",
    "yml", "toml", "ini", "cfg", "tsv", "png", "jpg", "jpeg", "gif", "webp",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentKind {
    Image,
    Text,
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    Unsupported,
}

pub fn document_kind_for_path(path: impl AsRef<Path>) -> DocumentKind {
    match path
        .as_ref()
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "gif" | "webp" => DocumentKind::Image,
        "txt" | "md" | "csv" | "json" | "xml" | "html" | "htm" | "log" | "yaml" | "yml"
        | "toml" | "ini" | "cfg" | "tsv" => DocumentKind::Text,
        "pdf" => DocumentKind::Pdf,
        "docx" => DocumentKind::Docx,
        "xlsx" => DocumentKind::Xlsx,
        "pptx" => DocumentKind::Pptx,
        _ => DocumentKind::Unsupported,
    }
}

pub trait DocumentExtractor {
    fn extract_text(&self, path: &Path, max_chars: usize) -> Result<Option<String>, String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FileDocumentExtractor;

impl DocumentExtractor for FileDocumentExtractor {
    fn extract_text(&self, path: &Path, max_chars: usize) -> Result<Option<String>, String> {
        extract_text(path, max_chars)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedDocumentExtractor;

impl DocumentExtractor for UnsupportedDocumentExtractor {
    fn extract_text(&self, _path: &Path, _max_chars: usize) -> Result<Option<String>, String> {
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedDocuments {
    pub text: String,
    pub image_paths: Vec<String>,
}

pub fn extract_text(path: impl AsRef<Path>, max_chars: usize) -> Result<Option<String>, String> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(Some(format!("[error: file not found: {}]", path.display())));
    }

    match document_kind_for_path(path) {
        DocumentKind::Text => read_text_file(path, max_chars),
        DocumentKind::Image => Ok(Some(format!(
            "[image: {}]",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
        ))),
        DocumentKind::Pdf => extract_pdf_text(path, max_chars),
        DocumentKind::Docx => extract_docx_text(path, max_chars),
        DocumentKind::Xlsx => extract_xlsx_text(path, max_chars),
        DocumentKind::Pptx => extract_pptx_text(path, max_chars),
        DocumentKind::Unsupported => Ok(None),
    }
}

pub fn extract_documents(
    text: &str,
    media_paths: &[String],
    max_file_size: u64,
) -> Result<ExtractedDocuments, String> {
    let mut image_paths = Vec::new();
    let mut doc_texts = Vec::new();

    for path_str in media_paths {
        let path = Path::new(path_str);
        if !path.is_file() {
            continue;
        }
        let metadata = match path.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.len() > max_file_size {
            continue;
        }
        let header = read_header(path, 16).unwrap_or_default();
        if detect_image_mime(&header).is_some()
            || document_kind_for_path(path) == DocumentKind::Image
        {
            image_paths.push(path_str.clone());
            continue;
        }
        if let Some(extracted) = extract_text(path, MAX_TEXT_LENGTH)? {
            if !extracted.starts_with("[error:") {
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path_str);
                doc_texts.push(format!("[File: {name}]\n{extracted}"));
            }
        }
    }

    let mut output = text.to_owned();
    if !doc_texts.is_empty() {
        output.push_str("\n\n");
        output.push_str(&doc_texts.join("\n\n"));
    }
    Ok(ExtractedDocuments {
        text: output,
        image_paths,
    })
}

fn read_text_file(path: &Path, max_chars: usize) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(truncate_with_total(&text, max_chars))),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            let text = bytes.into_iter().map(char::from).collect::<String>();
            Ok(Some(truncate_with_total(&text, max_chars)))
        }
        Err(error) => Ok(Some(format!("[error: failed to read file: {error}]"))),
    }
}

fn extract_pdf_text(path: &Path, max_chars: usize) -> Result<Option<String>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let source = String::from_utf8_lossy(&bytes);
    let mut parts = Vec::new();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '(' {
            continue;
        }
        let mut text = String::new();
        let mut escaped = false;
        for inner in chars.by_ref() {
            if escaped {
                text.push(match inner {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
                escaped = false;
            } else if inner == '\\' {
                escaped = true;
            } else if inner == ')' {
                break;
            } else {
                text.push(inner);
            }
        }
        let trimmed = text.trim();
        if trimmed.chars().any(|value| value.is_alphanumeric()) {
            parts.push(trimmed.to_owned());
        }
    }
    if parts.is_empty() {
        Ok(Some("[error: no extractable PDF text found]".to_owned()))
    } else {
        Ok(Some(truncate_with_total(&parts.join("\n"), max_chars)))
    }
}

fn extract_docx_text(path: &Path, max_chars: usize) -> Result<Option<String>, String> {
    let xml = read_zip_text_entries(path, |name| name == "word/document.xml")?;
    let text = xml.first().map(|entry| xml_text(entry)).unwrap_or_default();
    Ok(Some(truncate_with_total(&text, max_chars)))
}

fn extract_xlsx_text(path: &Path, max_chars: usize) -> Result<Option<String>, String> {
    let entries = read_zip_text_entries(path, |name| {
        name == "xl/sharedStrings.xml" || name.starts_with("xl/worksheets/sheet")
    })?;
    let mut parts = Vec::new();
    for entry in entries {
        let text = xml_text(&entry);
        if !text.is_empty() {
            parts.push(text);
        }
    }
    Ok(Some(truncate_with_total(&parts.join("\n"), max_chars)))
}

fn extract_pptx_text(path: &Path, max_chars: usize) -> Result<Option<String>, String> {
    let mut entries = read_zip_text_entries(path, |name| name.starts_with("ppt/slides/slide"))?;
    entries.sort();
    let mut parts = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let text = xml_text(entry);
        if !text.is_empty() {
            parts.push(format!("[Slide {}]\n{text}", index + 1));
        }
    }
    Ok(Some(truncate_with_total(&parts.join("\n\n"), max_chars)))
}

fn read_zip_text_entries(
    path: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|error| error.to_string())?;
        let name = file.name().to_owned();
        if !include(&name) {
            continue;
        }
        let mut text = String::new();
        file.read_to_string(&mut text)
            .map_err(|error| error.to_string())?;
        entries.push(text);
    }
    Ok(entries)
}

fn xml_text(xml: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    let mut entity = String::new();
    let mut in_entity = false;
    for ch in xml.chars() {
        if in_entity {
            if ch == ';' {
                output.push_str(match entity.as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
                entity.clear();
                in_entity = false;
            } else {
                entity.push(ch);
            }
            continue;
        }
        match ch {
            '<' => {
                in_tag = true;
                if !output.ends_with(['\n', ' ']) {
                    output.push(' ');
                }
            }
            '>' => in_tag = false,
            '&' if !in_tag => in_entity = true,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

fn truncate_with_total(text: &str, max_chars: usize) -> String {
    if max_chars == 0 || text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let total = text.chars().count();
    format!(
        "{}... (truncated, {total} chars total)",
        text.chars().take(max_chars).collect::<String>()
    )
}

fn read_header(path: &Path, max_bytes: usize) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    Ok(bytes.into_iter().take(max_bytes).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_supported_document_extensions() {
        assert_eq!(document_kind_for_path("a.PNG"), DocumentKind::Image);
        assert_eq!(document_kind_for_path("a.docx"), DocumentKind::Docx);
        assert_eq!(document_kind_for_path("a.txt"), DocumentKind::Text);
        assert_eq!(document_kind_for_path("a.toml"), DocumentKind::Text);
        assert_eq!(document_kind_for_path("a.bin"), DocumentKind::Unsupported);
    }

    #[test]
    fn extracts_text_images_and_missing_files() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-document-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let text_path = root.join("note.md");
        fs::write(&text_path, "hello document").map_err(|error| error.to_string())?;
        let image_path = root.join("pic.png");
        fs::write(&image_path, b"\x89PNG\r\n\x1a\nrest").map_err(|error| error.to_string())?;

        assert_eq!(
            extract_text(&text_path, MAX_TEXT_LENGTH)?,
            Some("hello document".to_owned())
        );
        assert!(extract_text(root.join("missing.md"), MAX_TEXT_LENGTH)?
            .unwrap_or_default()
            .starts_with("[error: file not found:"));
        let extracted = extract_documents(
            "base",
            &[
                text_path.to_string_lossy().to_string(),
                image_path.to_string_lossy().to_string(),
            ],
            MAX_EXTRACT_FILE_SIZE,
        )?;
        assert!(extracted.text.contains("[File: note.md]"));
        assert_eq!(
            extracted.image_paths,
            vec![image_path.to_string_lossy().to_string()]
        );
        Ok(())
    }

    #[test]
    fn extracts_best_effort_pdf_literal_text() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!(
            "shacs-utils-document-pdf-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| error.to_string())?
                .as_nanos()
        ));
        fs::create_dir_all(&root).map_err(|error| error.to_string())?;
        let path = root.join("sample.pdf");
        fs::write(&path, b"BT (Hello PDF) Tj (Second line) Tj ET")
            .map_err(|error| error.to_string())?;
        let extracted = extract_text(&path, MAX_TEXT_LENGTH)?.unwrap_or_default();
        assert!(extracted.contains("Hello PDF"));
        assert!(extracted.contains("Second line"));
        Ok(())
    }

    #[test]
    fn strips_xml_markup_for_office_text() {
        assert_eq!(
            xml_text("<w:t>Hello &amp; goodbye</w:t><w:t>Office</w:t>"),
            "Hello & goodbye Office"
        );
    }
}
