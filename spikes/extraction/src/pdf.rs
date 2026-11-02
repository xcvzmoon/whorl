use crate::{ExtractorTrial, OUTPUT_LIMIT, Result, picture};
use pdfium_render::prelude::*;
use serde_json::json;
use std::path::Path;

pub(crate) fn lopdf(path: &Path) -> Result<ExtractorTrial> {
    let document = lopdf::Document::load(path)?;
    let pages: Vec<u32> = document.get_pages().keys().copied().collect();
    if pages.len() > 100 {
        return Err("extraction_limit".into());
    }
    let mut trial = ExtractorTrial::default();
    trial.metadata.insert("pages".into(), json!(pages.len()));
    if let Ok(info) = document.trailer.get(b"Info") {
        let (_, info) = document.dereference(info)?;
        for (key, value) in info.as_dict()?.iter() {
            let (_, value) = document.dereference(value)?;
            if value.as_str().is_ok() {
                trial.metadata.insert(
                    String::from_utf8_lossy(key).to_lowercase(),
                    json!(lopdf::decode_text_string(value)?),
                );
            }
        }
    }
    trial.text = document.extract_text(&pages)?;
    Ok(trial)
}

pub(crate) fn pdfium(path: &Path, ocr: Option<&str>) -> Result<ExtractorTrial> {
    let library = std::env::var("PDFIUM_LIBRARY")?;
    let pdfium = Pdfium::new(Pdfium::bind_to_library(library)?);
    let document = pdfium.load_pdf_from_file(path, None)?;
    if document.pages().len() > 100 {
        return Err("extraction_limit".into());
    }
    let mut trial = ExtractorTrial::default();
    trial
        .metadata
        .insert("pages".into(), json!(document.pages().len()));
    for (tag, name) in [
        (PdfDocumentMetadataTagType::Title, "title"),
        (PdfDocumentMetadataTagType::Author, "author"),
        (PdfDocumentMetadataTagType::Subject, "subject"),
        (PdfDocumentMetadataTagType::Producer, "producer"),
        (PdfDocumentMetadataTagType::Creator, "creator"),
        (PdfDocumentMetadataTagType::CreationDate, "creationdate"),
        (PdfDocumentMetadataTagType::ModificationDate, "moddate"),
    ] {
        if let Some(value) = document.metadata().get(tag) {
            trial.metadata.insert(name.into(), json!(value.value()));
        }
    }
    for (index, page) in document.pages().iter().enumerate() {
        let text = if let Some(engine) = ocr {
            let width = (page.width().value * 300.0 / 72.0).ceil();
            let height = (page.height().value * 300.0 / 72.0).ceil();
            if !width.is_finite() || !height.is_finite() || width * height > 25_000_000.0 {
                return Err("extraction_limit".into());
            }
            let output = format!("page-{index}.png");
            page.render_with_config(&PdfRenderConfig::new().set_target_width(width as i32))?
                .as_image()?
                .save(&output)?;
            let result = picture::extract(Path::new(&output), engine);
            std::fs::remove_file(output)?;
            result?.text
        } else {
            page.text()?.all()
        };
        if trial.text.len() + text.len() + 1 > OUTPUT_LIMIT {
            return Err("extraction_limit".into());
        }
        trial.text.push_str(&text);
        trial.text.push('\n');
    }
    Ok(trial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_metadata_includes_indirect_producer() -> Result<()> {
        let trial = lopdf(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/scan.pdf")
                .as_path(),
        )?;
        assert_eq!(
            trial.metadata.get("producer"),
            Some(&json!("Mac OS X 10.10.4 Quartz PDFContext"))
        );
        Ok(())
    }
}
