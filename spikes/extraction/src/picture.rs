use crate::{ExtractorTrial, Result};
use image::{ImageReader, Limits};
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use serde_json::json;
use std::{fs::File, io::BufReader, path::Path, process::Command};

pub(crate) fn extract(path: &Path, engine: &str) -> Result<ExtractorTrial> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let dimensions = reader.into_dimensions()?;
    if u64::from(dimensions.0) * u64::from(dimensions.1) > 25_000_000 {
        return Err("extraction_limit".into());
    }
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode()?.into_rgb8();
    let mut trial = ExtractorTrial::default();
    trial.metadata.insert("width".into(), json!(dimensions.0));
    trial.metadata.insert("height".into(), json!(dimensions.1));
    match exif::Reader::new().read_from_container(&mut BufReader::new(File::open(path)?)) {
        Ok(exif) => {
            for field in exif.fields() {
                trial.metadata.insert(
                    format!("exif:{}", field.tag),
                    json!(field.display_value().with_unit(&exif).to_string()),
                );
            }
        }
        Err(exif::Error::NotFound(_)) => {}
        Err(error) => {
            trial
                .metadata
                .insert("exif_error".into(), json!(error.to_string()));
        }
    }
    trial.text = if engine.starts_with("ocrs") {
        let model_dir = std::env::var("OCR_MODELS")?;
        let filtered = engine == "ocrs-filtered";
        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(Model::load_file(
                Path::new(&model_dir).join("text-detection.onnx"),
            )?),
            recognition_model: Some(Model::load_file(
                Path::new(&model_dir).join("text-recognition.onnx"),
            )?),
            ..Default::default()
        })?;
        let input =
            engine.prepare_input(ImageSource::from_bytes(image.as_raw(), image.dimensions())?)?;
        let text = engine.get_text(&input)?;
        if filtered {
            text.lines()
                .filter(|line| line.chars().count() > 1)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text
        }
    } else {
        let output = Command::new("tesseract")
            .arg(path)
            .args([
                "stdout",
                "-l",
                "eng",
                "--psm",
                if engine == "tesseract-sparse" {
                    "11"
                } else {
                    "3"
                },
            ])
            .output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        String::from_utf8(output.stdout)?
    };
    Ok(trial)
}
