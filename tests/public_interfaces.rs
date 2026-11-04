use std::{fmt::Debug, num::NonZeroUsize, time::Duration};
use whorl::{
    cpu_pool::{CpuPool, WorkContext},
    extraction::{ExtractionLimits, Extractor},
    queue::JobQueue,
    service::matching::{Matcher, PreparedDocument},
    similarity::{shingles::SourceProfile, signature::SignatureVersion},
    storage::UploadStore,
};

#[test]
fn exported_handles_support_debug() {
    fn debug<T: Debug>() {}
    debug::<CpuPool>();
    debug::<WorkContext>();
    debug::<PreparedDocument>();
    debug::<Matcher>();
    debug::<Extractor>();
    debug::<UploadStore>();
    debug::<JobQueue>();
}

#[test]
fn prepared_document_debug_excludes_text_and_metadata() {
    let document = PreparedDocument::new(
        "confidential document marker",
        serde_json::json!({"author":"confidential metadata marker"}),
        SourceProfile::NativeTextV1,
        None,
    )
    .unwrap();
    for rendered in [format!("{document:?}"), format!("{document:#?}")] {
        assert!(rendered.contains("PreparedDocument"));
        assert!(rendered.contains(document.signature_version()));
        assert!(!rendered.contains("confidential"));
        assert!(!rendered.contains("author"));
    }
}

#[tokio::test]
async fn handles_do_not_debug_database_or_storage_credentials() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://credential-user:database-secret-marker@localhost/unused")
        .unwrap();
    let s3 = object_store::aws::AmazonS3Builder::new()
        .with_endpoint("https://storage.example.com")
        .with_bucket_name("private-bucket-marker")
        .with_region("us-east-1")
        .with_access_key_id("access-key-marker")
        .with_secret_access_key("secret-key-marker")
        .build()
        .unwrap();
    let store = UploadStore::new(pool.clone(), s3);
    let queue = JobQueue::new(pool, std::num::NonZeroU16::MIN);
    for rendered in [format!("{store:?}"), format!("{queue:?}")] {
        for marker in [
            "credential-user",
            "database-secret-marker",
            "private-bucket-marker",
            "access-key-marker",
            "secret-key-marker",
        ] {
            assert!(!rendered.contains(marker), "Debug exposed {marker}");
        }
    }
    let extractor = Extractor::new(
        "/private/extraction-path-marker".into(),
        ExtractionLimits::default(),
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(!format!("{extractor:?}").contains("extraction-path-marker"));
}

#[test]
fn signature_display_is_the_canonical_persistence_key() {
    for source in [SourceProfile::NativeTextV1, SourceProfile::OcrVerbatimV1] {
        let version = SignatureVersion::new(source, NonZeroUsize::new(4).unwrap());
        assert_eq!(version.to_string(), version.as_str());
    }
}
