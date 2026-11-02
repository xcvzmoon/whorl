mod office;
mod pdf;
mod picture;

use serde::Serialize;
use serde_json::{Map, Value};
use std::{error::Error, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const INPUT_LIMIT: u64 = 20 * 1024 * 1024;
const OUTPUT_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Default, Serialize)]
struct ExtractorTrial {
    text: String,
    metadata: Map<String, Value>,
    outcome: String,
    error: Option<String>,
}

fn extract(candidate: &str, format: &str, path: &Path) -> Result<ExtractorTrial> {
    if path.metadata()?.len() > INPUT_LIMIT {
        return Err("file_too_large".into());
    }
    let mut trial = match (candidate, format) {
        ("lopdf", "pdf") => pdf::lopdf(path)?,
        ("pdfium", "pdf") => pdf::pdfium(path, None)?,
        ("pdfium-ocrs", "pdf") => pdf::pdfium(path, Some("ocrs"))?,
        ("pdfium-tesseract", "pdf") => pdf::pdfium(path, Some("tesseract"))?,
        ("ooxml", "docx" | "pptx") => office::document(path, format)?,
        ("calamine", "xlsx") => office::spreadsheet(path)?,
        ("image-ocrs", "png" | "jpeg") => picture::extract(path, "ocrs")?,
        ("image-ocrs-filtered", "png" | "jpeg") => picture::extract(path, "ocrs-filtered")?,
        ("image-tesseract-sparse", "png" | "jpeg") => picture::extract(path, "tesseract-sparse")?,
        ("image-tesseract", "png" | "jpeg") => picture::extract(path, "tesseract")?,
        _ => return Err("unsupported_format".into()),
    };
    if trial.text.len() > OUTPUT_LIMIT {
        return Err("extraction_limit".into());
    }
    trial.outcome = if trial.text.trim().is_empty() {
        "no_extractable_text"
    } else {
        "success"
    }
    .into();
    Ok(trial)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: extraction-spike CANDIDATE FORMAT PATH".into());
    }
    let trial = extract(&args[1], &args[2], Path::new(&args[3])).unwrap_or_else(|error| {
        let message = error.to_string();
        let outcome = match message.as_str() {
            "file_too_large" | "extraction_limit" | "unsupported_format" => message.clone(),
            _ => "extraction_failed".into(),
        };
        ExtractorTrial {
            outcome,
            error: Some(message),
            ..Default::default()
        }
    });
    println!("{}", serde_json::to_string(&trial)?);
    Ok(())
}
