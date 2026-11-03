//! Content-detected extraction in supervised Linux subprocesses.

mod limits;
mod native;
mod office;
mod supervisor;

pub use limits::ExtractionLimits;
pub use supervisor::Extractor;

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

/// Text and metadata recovered from one input before normalization.
#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractedDocument {
    /// Body text in document order, including presentation notes.
    pub text: String,
    /// Format properties and embedded metadata, without filesystem provenance.
    pub metadata: BTreeMap<String, String>,
    /// Whether OCR contributed any text.
    pub ocr: bool,
}

/// Stable per-file failures from detection, extraction or process supervision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ExtractionError {
    /// The input has no bytes (`empty_file`).
    #[error("empty_file")]
    EmptyFile,
    /// Input exceeds the configured byte cap (`file_too_large`).
    #[error("file_too_large")]
    FileTooLarge,
    /// Content has no supported signature (`unsupported_type`).
    #[error("unsupported_type")]
    UnsupportedMediaType,
    /// A recognized format is damaged or structurally invalid (`malformed_file`).
    #[error("malformed_file")]
    MalformedFile,
    /// A parsing, expansion, pixel, text or process resource ceiling was exceeded (`extraction_limit`).
    #[error("extraction_limit")]
    LimitExceeded,
    /// The wall deadline expired (`extraction_timeout`).
    #[error("extraction_timeout")]
    Timeout,
    /// Extraction produced only whitespace (`no_extractable_text`).
    #[error("no_extractable_text")]
    NoExtractableText,
    /// Runtime tools, I/O, configuration or child protocol failed (`extraction_failed`).
    #[error("extraction_failed")]
    ExtractionFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    Pdf,
    Png,
    Jpeg,
    Docx,
    Pptx,
    Xlsx,
}

fn detect(path: &Path, limits: &ExtractionLimits) -> Result<Format, ExtractionError> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|_| ExtractionError::ExtractionFailed)?;
    let mut magic = [0; 8];
    let size = file
        .read(&mut magic)
        .map_err(|_| ExtractionError::ExtractionFailed)?;
    let magic = &magic[..size];
    if magic.starts_with(b"%PDF-") {
        return Ok(Format::Pdf);
    }
    if magic.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(Format::Png);
    }
    if magic.starts_with(b"\xff\xd8\xff") {
        return Ok(Format::Jpeg);
    }
    if magic.starts_with(b"PK") {
        return office::detect(path, limits);
    }
    Err(ExtractionError::UnsupportedMediaType)
}

/// Runs the private extraction-worker protocol on standard input and output.
///
/// Only the `whorl-extract` executable should call this entry point. Run it via
/// [`Extractor`] so input, process resources, deadlines and output are bounded.
///
/// # Returns
/// Nothing. A single JSON result is written to stdout, including per-file errors.
///
/// # Examples
/// ```no_run
/// whorl::extraction::run_worker();
/// ```
#[doc(hidden)]
pub fn run_worker() {
    fn run() -> Result<ExtractedDocument, ExtractionError> {
        use std::io::Read;
        let mut input = String::new();
        std::io::stdin()
            .take(16 * 1024)
            .read_to_string(&mut input)
            .map_err(|_| ExtractionError::ExtractionFailed)?;
        let limits: ExtractionLimits =
            serde_json::from_str(&input).map_err(|_| ExtractionError::ExtractionFailed)?;
        let path = std::env::args_os()
            .nth(1)
            .ok_or(ExtractionError::ExtractionFailed)?;
        let path = Path::new(&path);
        let bytes = path
            .metadata()
            .map_err(|_| ExtractionError::ExtractionFailed)?
            .len();
        if bytes == 0 {
            return Err(ExtractionError::EmptyFile);
        }
        if bytes > limits.max_input_bytes {
            return Err(ExtractionError::FileTooLarge);
        }
        let format = detect(path, &limits)?;
        let document = match format {
            Format::Docx | Format::Pptx | Format::Xlsx => office::extract(path, format, &limits)?,
            _ => native::extract(path, format, &limits)?,
        };
        if document.text.len() > limits.max_text_bytes {
            return Err(ExtractionError::LimitExceeded);
        }
        if document.text.trim().is_empty() {
            return Err(ExtractionError::NoExtractableText);
        }
        Ok(document)
    }
    if let Ok(json) = serde_json::to_string(&run()) {
        println!("{json}");
    }
}
