use super::{ExtractedDocument, ExtractionError, ExtractionLimits, Format};
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use pdfium_render::prelude::*;
use rten::Model;
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    process::{Command, Stdio},
};

pub(super) fn extract(
    path: &Path,
    format: Format,
    limits: &ExtractionLimits,
) -> Result<ExtractedDocument, ExtractionError> {
    match format {
        Format::Pdf => pdf(path, limits),
        Format::Png | Format::Jpeg => picture(path, format, limits),
        _ => Err(ExtractionError::UnsupportedMediaType),
    }
}

fn append(text: &mut String, value: &str, limit: usize) -> Result<(), ExtractionError> {
    if text.len().saturating_add(value.len()).saturating_add(1) > limit {
        return Err(ExtractionError::LimitExceeded);
    }
    text.push_str(value);
    text.push('\n');
    Ok(())
}

fn finish(
    document: ExtractedDocument,
    limits: &ExtractionLimits,
) -> Result<ExtractedDocument, ExtractionError> {
    let metadata_bytes = document
        .metadata
        .iter()
        .map(|(k, v)| k.len().saturating_add(v.len()))
        .try_fold(0usize, |total, size| total.checked_add(size))
        .ok_or(ExtractionError::LimitExceeded)?;
    if document.text.len().saturating_add(metadata_bytes) > limits.max_text_bytes {
        return Err(ExtractionError::LimitExceeded);
    }
    if document.text.trim().is_empty() {
        return Err(ExtractionError::NoExtractableText);
    }
    Ok(document)
}

fn picture(
    path: &Path,
    format: Format,
    limits: &ExtractionLimits,
) -> Result<ExtractedDocument, ExtractionError> {
    let reader = ImageReader::open(path)
        .map_err(|_| ExtractionError::ExtractionFailed)?
        .with_guessed_format()
        .map_err(|_| ExtractionError::MalformedFile)?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| ExtractionError::MalformedFile)?;
    check_pixels(width, height, limits)?;
    let mut reader = ImageReader::open(path)
        .map_err(|_| ExtractionError::ExtractionFailed)?
        .with_guessed_format()
        .map_err(|_| ExtractionError::MalformedFile)?;
    let mut allocation_limits = Limits::default();
    allocation_limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(allocation_limits);
    let mut decoder = reader.into_decoder().map_err(image_error)?;
    if decoder.total_bytes() > 256 * 1024 * 1024 {
        return Err(ExtractionError::LimitExceeded);
    }
    let orientation = decoder.orientation().map_err(image_error)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(image_error)?;
    image.apply_orientation(orientation);
    let image = image.into_rgb8();
    let mut metadata = BTreeMap::from([
        ("width".into(), width.to_string()),
        ("height".into(), height.to_string()),
    ]);
    exif(path, &mut metadata)?;
    let detection =
        std::env::var_os("WHORL_OCR_DETECTION_MODEL").ok_or(ExtractionError::ExtractionFailed)?;
    let recognition =
        std::env::var_os("WHORL_OCR_RECOGNITION_MODEL").ok_or(ExtractionError::ExtractionFailed)?;
    let engine = OcrEngine::new(OcrEngineParams {
        detection_model: Some(
            Model::load_file(Path::new(&detection))
                .map_err(|_| ExtractionError::ExtractionFailed)?,
        ),
        recognition_model: Some(
            Model::load_file(Path::new(&recognition))
                .map_err(|_| ExtractionError::ExtractionFailed)?,
        ),
        ..Default::default()
    })
    .map_err(|_| ExtractionError::ExtractionFailed)?;
    let input = engine
        .prepare_input(
            ImageSource::from_bytes(image.as_raw(), image.dimensions())
                .map_err(|_| ExtractionError::ExtractionFailed)?,
        )
        .map_err(|_| ExtractionError::ExtractionFailed)?;
    let text = engine
        .get_text(&input)
        .map_err(|_| ExtractionError::ExtractionFailed)?;
    finish(
        ExtractedDocument {
            file_type: format.as_str().into(),
            text,
            metadata,
            ocr: true,
        },
        limits,
    )
}

fn image_error(error: image::ImageError) -> ExtractionError {
    match error {
        image::ImageError::Limits(_) => ExtractionError::LimitExceeded,
        _ => ExtractionError::MalformedFile,
    }
}

fn check_pixels(width: u32, height: u32, limits: &ExtractionLimits) -> Result<(), ExtractionError> {
    if width == 0 || height == 0 {
        return Err(ExtractionError::MalformedFile);
    }
    if u64::from(width) * u64::from(height) > limits.max_image_pixels {
        return Err(ExtractionError::LimitExceeded);
    }
    Ok(())
}

fn exif(path: &Path, metadata: &mut BTreeMap<String, String>) -> Result<(), ExtractionError> {
    match nom_exif::read_exif(path) {
        Ok(exif) => {
            if !exif.errors().is_empty() {
                return Err(ExtractionError::MalformedFile);
            }
            for entry in exif.entries() {
                if let Some(tag) = entry.tag().tag() {
                    metadata.insert(format!("exif:{tag}"), entry.value().to_string());
                }
            }
            Ok(())
        }
        Err(nom_exif::Error::ExifNotFound) => Ok(()),
        Err(_) => Err(ExtractionError::MalformedFile),
    }
}

fn pdf(path: &Path, limits: &ExtractionLimits) -> Result<ExtractedDocument, ExtractionError> {
    let library =
        std::env::var_os("WHORL_PDFIUM_LIBRARY").ok_or(ExtractionError::ExtractionFailed)?;
    let pdfium = Pdfium::new(
        Pdfium::bind_to_library(Path::new(&library))
            .map_err(|_| ExtractionError::ExtractionFailed)?,
    );
    let document = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|_| ExtractionError::MalformedFile)?;
    if usize::try_from(document.pages().len()).map_err(|_| ExtractionError::MalformedFile)?
        > limits.max_pages
    {
        return Err(ExtractionError::LimitExceeded);
    }
    let mut output = ExtractedDocument {
        file_type: "pdf".into(),
        text: String::new(),
        metadata: BTreeMap::from([("pages".into(), document.pages().len().to_string())]),
        ocr: false,
    };
    for (tag, key) in [
        (PdfDocumentMetadataTagType::Title, "title"),
        (PdfDocumentMetadataTagType::Author, "author"),
        (PdfDocumentMetadataTagType::Subject, "subject"),
        (PdfDocumentMetadataTagType::Producer, "producer"),
        (PdfDocumentMetadataTagType::Creator, "creator"),
        (PdfDocumentMetadataTagType::CreationDate, "creationdate"),
        (PdfDocumentMetadataTagType::ModificationDate, "moddate"),
    ] {
        if let Some(value) = document.metadata().get(tag) {
            output
                .metadata
                .insert(key.into(), value.value().to_string());
        }
    }
    for page in document.pages().iter() {
        let native_text = page
            .text()
            .map_err(|_| ExtractionError::MalformedFile)?
            .all();
        if native_text.chars().any(char::is_alphanumeric) {
            append(&mut output.text, &native_text, limits.max_text_bytes)?;
            continue;
        }
        let width = (page.width().value * 300.0 / 72.0).ceil();
        let height = (page.height().value * 300.0 / 72.0).ceil();
        if !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
            || width > i32::MAX as f32
            || height > i32::MAX as f32
            || f64::from(width) * f64::from(height) > limits.max_image_pixels as f64
        {
            return Err(ExtractionError::LimitExceeded);
        }
        let raster = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .map_err(|_| ExtractionError::ExtractionFailed)?;
        page.render_with_config(&PdfRenderConfig::new().set_target_width(width as i32))
            .map_err(|_| ExtractionError::ExtractionFailed)?
            .as_image()
            .map_err(|_| ExtractionError::ExtractionFailed)?
            .save(raster.path())
            .map_err(|_| ExtractionError::ExtractionFailed)?;
        let remaining = limits.max_text_bytes.saturating_sub(output.text.len());
        let text = tesseract(raster.path(), remaining)?;
        append(&mut output.text, &text, limits.max_text_bytes)?;
        output.ocr = true;
    }
    finish(output, limits)
}

fn tesseract(path: &Path, limit: usize) -> Result<String, ExtractionError> {
    let mut child = Command::new("tesseract")
        .arg(path)
        .args(["stdout", "-l", "eng", "--psm", "3"])
        .env("OMP_THREAD_LIMIT", "2")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ExtractionError::ExtractionFailed)?;
    let result = (|| {
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or(ExtractionError::ExtractionFailed)?
            .take(limit.saturating_add(1) as u64)
            .read_to_end(&mut output)
            .map_err(|_| ExtractionError::ExtractionFailed)?;
        if output.len() > limit {
            return Err(ExtractionError::LimitExceeded);
        }
        if !child
            .wait()
            .map_err(|_| ExtractionError::ExtractionFailed)?
            .success()
        {
            return Err(ExtractionError::ExtractionFailed);
        }
        String::from_utf8(output).map_err(|_| ExtractionError::ExtractionFailed)
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_limit_includes_separator() {
        let mut text = String::new();
        assert!(append(&mut text, "ab", 2).is_err());
        append(&mut text, "ab", 3).unwrap();
        assert_eq!(text, "ab\n");
    }

    #[test]
    fn reads_populated_jpeg_exif_and_accepts_absent_exif() {
        use std::io::Write;
        let mut file = tempfile::Builder::new().suffix(".jpg").tempfile().unwrap();
        let mut tiff = Vec::from(&b"II\x2a\x00\x08\x00\x00\x00"[..]);
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x010fu16.to_le_bytes());
        tiff.extend_from_slice(&2u16.to_le_bytes());
        tiff.extend_from_slice(&5u32.to_le_bytes());
        tiff.extend_from_slice(&26u32.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        tiff.extend_from_slice(b"ACME\0");
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&u16::try_from(tiff.len() + 8).unwrap().to_be_bytes());
        jpeg.extend_from_slice(b"Exif\0\0");
        jpeg.extend_from_slice(&tiff);
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2, 2)
            .write_to(&mut encoded, image::ImageFormat::Jpeg)
            .unwrap();
        jpeg.extend_from_slice(&encoded.into_inner()[2..]);
        file.write_all(&jpeg).unwrap();
        let mut metadata = BTreeMap::new();
        exif(file.path(), &mut metadata).unwrap();
        assert_eq!(metadata.get("exif:Make").map(String::as_str), Some("ACME"));
        let image_file = tempfile::Builder::new().suffix(".png").tempfile().unwrap();
        image::RgbImage::new(2, 2).save(image_file.path()).unwrap();
        metadata.clear();
        exif(image_file.path(), &mut metadata).unwrap();
        assert!(metadata.is_empty());
        for name in ["stop.jpg", "map.png"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("spikes/extraction/fixtures")
                .join(name);
            exif(&path, &mut BTreeMap::new()).unwrap();
        }
    }

    #[test]
    fn rejects_zero_and_excessive_image_dimensions() {
        let limits = ExtractionLimits::default();
        assert!(check_pixels(0, 1, &limits).is_err());
        assert!(check_pixels(10_000, 10_000, &limits).is_err());
        assert!(check_pixels(100, 100, &limits).is_ok());
    }
}
