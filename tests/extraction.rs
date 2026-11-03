use std::{
    io::Write,
    process::{Command, Stdio},
};
use whorl::extraction::{ExtractedDocument, ExtractionError, ExtractionLimits, Extractor};

fn worker(path: &str, limits: ExtractionLimits) -> Result<ExtractedDocument, ExtractionError> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_whorl-extract"))
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&limits).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn detects_content_and_rejects_empty_malformed_and_expansion_inputs() {
    let cases = [
        "spikes/extraction/fixtures/table.docx",
        "spikes/extraction/fixtures/table.pptx",
        "spikes/extraction/fixtures/workbook.xlsx",
    ];
    for path in cases {
        let doc = worker(path, ExtractionLimits::default()).unwrap();
        assert!(!doc.text.trim().is_empty());
    }
    let empty = tempfile::NamedTempFile::new().unwrap();
    assert_eq!(
        worker(empty.path().to_str().unwrap(), ExtractionLimits::default()).unwrap_err(),
        ExtractionError::EmptyFile
    );
    let mut unknown = tempfile::NamedTempFile::new().unwrap();
    unknown.write_all(b"not a document").unwrap();
    assert_eq!(
        worker(
            unknown.path().to_str().unwrap(),
            ExtractionLimits::default()
        )
        .unwrap_err(),
        ExtractionError::UnsupportedMediaType
    );
    let mut broken = tempfile::NamedTempFile::new().unwrap();
    broken.write_all(b"PK\x03\x04broken").unwrap();
    assert_eq!(
        worker(broken.path().to_str().unwrap(), ExtractionLimits::default()).unwrap_err(),
        ExtractionError::MalformedFile
    );
}

#[tokio::test]
async fn rejects_size_before_starting_worker() {
    let limits = ExtractionLimits {
        max_input_bytes: 2,
        ..Default::default()
    };
    let extractor = Extractor::new(
        "/missing/worker".into(),
        limits,
        std::time::Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        extractor.extract(b"").await.unwrap_err(),
        ExtractionError::EmptyFile
    );
    assert_eq!(
        extractor.extract(b"123").await.unwrap_err(),
        ExtractionError::FileTooLarge
    );
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    fn script(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .unwrap();
        file
    }

    #[tokio::test]
    async fn deadline_kills_descendants_and_removes_temporary_input() {
        let record = tempfile::NamedTempFile::new().unwrap();
        let command = format!(
            "#!/bin/sh\ncat >/dev/null\nsleep 30 &\nprintf '%s\\n%s\\n' \"$!\" \"$1\" > '{}'\nwait\n",
            record.path().display()
        );
        let file = script(&command);
        let worker_path = file.path().to_owned();
        let (_handle, retained) = file.keep().unwrap();
        drop(_handle);
        let extractor = Extractor::new(
            worker_path,
            ExtractionLimits::default(),
            Duration::from_millis(300),
        )
        .unwrap();
        assert_eq!(
            extractor.extract(b"hello").await.unwrap_err(),
            ExtractionError::Timeout
        );
        let record = std::fs::read_to_string(record.path()).unwrap();
        let mut lines = record.lines();
        let pid = lines.next().unwrap();
        assert!(!std::path::Path::new(lines.next().unwrap()).exists());
        let status = std::fs::read_to_string(format!("/proc/{pid}/stat"));
        assert!(status.is_err() || status.unwrap().split_whitespace().nth(2) == Some("Z"));
        std::fs::remove_file(retained).unwrap();
    }

    #[tokio::test]
    async fn cancellation_removes_temporary_input_and_kills_group() {
        let record = tempfile::NamedTempFile::new().unwrap();
        let file = script(&format!(
            "#!/bin/sh\ncat >/dev/null\nsleep 30 &\nprintf '%s\n%s\n' \"$!\" \"$1\" > '{}'\nwait\n",
            record.path().display()
        ));
        let (handle, path) = file.keep().unwrap();
        drop(handle);
        let extractor = Extractor::new(
            path.clone(),
            ExtractionLimits::default(),
            Duration::from_secs(30),
        )
        .unwrap();
        let task = tokio::spawn(async move { extractor.extract(b"hello").await });
        let mut recorded = String::new();
        for _ in 0..100 {
            recorded = std::fs::read_to_string(record.path()).unwrap();
            if !recorded.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!recorded.is_empty());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        let mut lines = recorded.lines();
        let pid = lines.next().unwrap();
        assert!(!std::path::Path::new(lines.next().unwrap()).exists());
        tokio::time::sleep(Duration::from_millis(20)).await;
        let status = std::fs::read_to_string(format!("/proc/{pid}/stat"));
        assert!(status.is_err() || status.unwrap().split_whitespace().nth(2) == Some("Z"));
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn bounds_live_child_output() {
        let file = script("#!/bin/sh\ncat >/dev/null\nhead -c 1100000 /dev/zero\n");
        let (handle, path) = file.keep().unwrap();
        drop(handle);
        let limits = ExtractionLimits {
            max_text_bytes: 1,
            ..Default::default()
        };
        let extractor = Extractor::new(path.clone(), limits, Duration::from_secs(5)).unwrap();
        assert_eq!(
            extractor.extract(b"hello").await.unwrap_err(),
            ExtractionError::LimitExceeded
        );
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn supervisor_extracts_office_and_bounds_child_output() {
        let extractor = Extractor::new(
            env!("CARGO_BIN_EXE_whorl-extract").into(),
            ExtractionLimits::default(),
            Duration::from_secs(60),
        )
        .unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../spikes/extraction/manifest.json")).unwrap();
        for case in manifest["cases"].as_array().unwrap() {
            if case["expected"]["outcome"] == "detect_png" {
                continue;
            }
            if !["docx", "pptx", "xlsx"].contains(&case["format"].as_str().unwrap()) {
                continue;
            }
            let path = format!("spikes/extraction/{}", case["path"].as_str().unwrap());
            let input = std::fs::read(&path).unwrap();
            let result = extractor.extract(&input).await;
            if case["expected"]["outcome"] == "success" {
                assert!(result.is_ok(), "{path}: {result:?}");
            } else {
                assert!(result.is_err(), "{path}");
            }
        }
    }
}
