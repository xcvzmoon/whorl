use object_store::{ObjectStoreExt, memory::InMemory, path::Path};
use serde_json::json;
use sqlx::{PgPool, Row};
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use uuid::Uuid;
use whorl::{
    extraction::{ExtractionLimits, Extractor},
    queue::FileJobPayload,
    service::{
        matching::PreparedDocument,
        process_file::{ProcessError, ProcessFile, WorkerConfig},
    },
    similarity::{shingles::SourceProfile, signature::SignatureVersion},
};

fn config() -> WorkerConfig {
    WorkerConfig {
        workers: NonZeroUsize::MIN,
        retention: Duration::from_secs(3600),
        tombstone_retention: Duration::from_secs(3600),
    }
}

#[tokio::test]
async fn rejects_invalid_worker_bounds() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_lazy("postgresql://localhost/unused")
        .unwrap();
    let extractor = || {
        Extractor::new(
            "/missing/extractor".into(),
            ExtractionLimits::default(),
            Duration::from_secs(1),
        )
        .unwrap()
    };
    let mut invalid = config();
    invalid.workers = NonZeroUsize::new(3).unwrap();
    assert!(matches!(
        ProcessFile::new(
            pool.clone(),
            Arc::new(InMemory::new()),
            extractor(),
            invalid
        ),
        Err(ProcessError::Invalid)
    ));
    invalid = config();
    invalid.retention = Duration::ZERO;
    assert!(matches!(
        ProcessFile::new(pool, Arc::new(InMemory::new()), extractor(), invalid),
        Err(ProcessError::Invalid)
    ));
}

async fn seed(pool: &PgPool, objects: &InMemory, mode: &str, bytes: &[u8]) -> FileJobPayload {
    let batch = Uuid::new_v4();
    let file = Uuid::new_v4();
    let upload = Uuid::new_v4();
    let key = format!("uploads/{upload}");
    objects
        .put(&Path::from(key.clone()), bytes.to_vec().into())
        .await
        .unwrap();
    let version =
        PreparedDocument::new("hello world", json!({}), SourceProfile::NativeTextV1, None)
            .unwrap()
            .signature_version()
            .to_owned();
    sqlx::query("INSERT INTO signature_versions(version,configuration) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(&version).bind(version.parse::<SignatureVersion>().unwrap().configuration()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO capacity_reservations(id,client_id,jobs,bytes,lease_until) VALUES($1,'worker',1,$2,clock_timestamp()+interval '1 hour')").bind(upload).bind(i64::try_from(bytes.len()).unwrap()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO stored_uploads(id,client_id,object_key,bytes,expires_at) VALUES($1,'worker',$2,$3,clock_timestamp()+interval '1 hour')").bind(upload).bind(&key).bind(i64::try_from(bytes.len()).unwrap()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES($1,'worker',$2,$3,$4,clock_timestamp()+interval '1 hour')").bind(batch).bind(mode).bind(version).bind(json!({"kind":"text_only","threshold":0.75})).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at,source_object_key,source_bytes) VALUES($1,$2,'worker',0,'misleading.exe',clock_timestamp()+interval '1 hour',$3,$4)").bind(file).bind(batch).bind(key).bind(i64::try_from(bytes.len()).unwrap()).execute(pool).await.unwrap();
    FileJobPayload {
        client_id: "worker".into(),
        batch_id: batch,
        file_id: file,
    }
}

async fn state(pool: &PgPool, payload: &FileJobPayload) -> String {
    sqlx::query_scalar("SELECT state FROM batch_files WHERE id=$1")
        .bind(payload.file_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires fresh PostgreSQL database in WHORL_WORKER_TEST_URL; Linux exercises successful extraction"]
#[expect(
    clippy::too_many_lines,
    reason = "One fresh database scenario covers durable processing and recovery"
)]
async fn durable_processing_and_recovery() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&std::env::var("WHORL_WORKER_TEST_URL").unwrap())
        .await
        .unwrap();
    #[cfg(target_os = "linux")]
    if let Ok(file) = std::env::var("WHORL_WORKER_CHILD_FILE") {
        run_child_worker(&pool, Uuid::parse_str(&file).unwrap()).await;
        return;
    }
    for migration in [
        include_str!("../database/migrations/0001_application.sql"),
        include_str!("../database/migrations/0002_uploads.sql"),
    ] {
        sqlx::raw_sql(migration).execute(&pool).await.unwrap();
    }
    let objects = Arc::new(InMemory::new());
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects.clone(),
        Extractor::new(
            std::path::PathBuf::from(env!("CARGO_BIN_EXE_whorl-extract")),
            ExtractionLimits::default(),
            Duration::from_secs(10),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    let bytes = std::fs::read("spikes/extraction/fixtures/table.docx").unwrap();
    let payload = seed(&pool, &objects, "ingest", &bytes).await;
    let mut foreign = payload.clone();
    foreign.client_id = "another".into();
    pipeline.process(&foreign, false).await.unwrap();
    assert_eq!(state(&pool, &payload).await, "queued");
    let mut fence = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,15))")
        .bind(payload.file_id.to_string())
        .execute(&mut *fence)
        .await
        .unwrap();
    assert!(matches!(
        pipeline.process(&payload, false).await,
        Err(ProcessError::Busy)
    ));
    fence.rollback().await.unwrap();
    pipeline.process(&payload, false).await.unwrap();
    let expected = if cfg!(target_os = "linux") {
        "completed"
    } else {
        "failed"
    };
    assert_eq!(state(&pool, &payload).await, expected);
    let attempt: i32 = sqlx::query_scalar("SELECT attempt FROM batch_files WHERE id=$1")
        .bind(payload.file_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    pipeline.process(&payload, false).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT attempt FROM batch_files WHERE id=$1")
            .bind(payload.file_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        attempt
    );
    let terminal = sqlx::query("SELECT state,completed_at IS NOT NULL AND expires_at>completed_at AND tombstone_until>expires_at AS retained FROM batches WHERE id=$1").bind(payload.batch_id).fetch_one(&pool).await.unwrap();
    assert_eq!(terminal.get::<String, _>("state"), expected);
    assert!(terminal.get::<bool, _>("retained"));
    if cfg!(target_os = "linux") {
        let check = seed(&pool, &objects, "check", &bytes).await;
        pipeline.process(&check, false).await.unwrap();
        assert_eq!(state(&pool, &check).await, "completed");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM match_results WHERE file_id=$1")
                .bind(check.file_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        let duplicate = seed(&pool, &objects, "ingest", &bytes).await;
        pipeline.process(&duplicate, false).await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM documents")
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM match_results WHERE file_id=$1")
                .bind(duplicate.file_id)
                .fetch_one(&pool)
                .await
                .unwrap(),
            1
        );
    }
    let missing = seed(&pool, &objects, "check", &bytes).await;
    let key: String = sqlx::query_scalar("SELECT source_object_key FROM batch_files WHERE id=$1")
        .bind(missing.file_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    objects.delete(&Path::from(key)).await.unwrap();
    assert!(matches!(
        pipeline.process(&missing, false).await,
        Err(ProcessError::Storage(_))
    ));
    assert_eq!(state(&pool, &missing).await, "queued");
    let mut retry = pool.begin().await.unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock(hashtextextended($1,15))")
            .bind(missing.file_id.to_string())
            .fetch_one(&mut *retry)
            .await
            .unwrap()
    );
    retry.rollback().await.unwrap();
    pipeline.process(&missing, true).await.unwrap();
    assert_eq!(state(&pool, &missing).await, "failed");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT error->>'code' FROM batch_files WHERE id=$1")
            .bind(missing.file_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "retry_exhausted"
    );
    let expired = seed(&pool, &objects, "check", &bytes).await;
    sqlx::query(
        "UPDATE batch_files SET deadline_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(expired.file_id)
    .execute(&pool)
    .await
    .unwrap();
    pipeline.process(&expired, false).await.unwrap();
    assert_eq!(state(&pool, &expired).await, "failed");
    let invalid = seed(&pool, &objects, "check", b"unsupported input").await;
    pipeline.process(&invalid, false).await.unwrap();
    assert_eq!(state(&pool, &invalid).await, "failed");
    pipeline.shutdown().await;
    #[cfg(target_os = "linux")]
    {
        cancelled_attempt_restarts_without_duplicate_results().await;
        mixed_source_namespaces(&pool).await;
        maintenance_registered_nondefault_versions(&pool).await;
        queue_shutdown_and_live_reclaim_protection(&pool).await;
        killed_worker_recovers(&pool).await;
        handler_concurrency_is_bounded(&pool).await;
    }
}

#[cfg(target_os = "linux")]
async fn cancelled_attempt_restarts_without_duplicate_results() {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(12)
        .connect(&std::env::var("WHORL_WORKER_TEST_URL").unwrap())
        .await
        .unwrap();
    let objects = Arc::new(InMemory::new());
    let mut script = tempfile::NamedTempFile::new().unwrap();
    writeln!(script,"#!/bin/sh\ncat >/dev/null\nsleep 2\nprintf '%s' '{{\"Ok\":{{\"file_type\":\"docx\",\"text\":\"recovered synthetic text\",\"metadata\":{{}},\"ocr\":false}}}}'").unwrap();
    script
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .unwrap();
    let script = script.into_temp_path();
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects.clone(),
        Extractor::new(
            script.to_path_buf(),
            ExtractionLimits::default(),
            Duration::from_secs(10),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    let payload = seed(&pool, &objects, "ingest", b"synthetic").await;
    assert!(
        tokio::time::timeout(
            Duration::from_millis(200),
            pipeline.process(&payload, false)
        )
        .await
        .is_err()
    );
    assert_eq!(state(&pool, &payload).await, "running");
    tokio::time::sleep(Duration::from_millis(150)).await;
    pipeline.process(&payload, false).await.unwrap();
    assert_eq!(state(&pool, &payload).await, "completed");
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT attempt FROM batch_files WHERE id=$1")
            .bind(payload.file_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    pipeline.process(&payload, false).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM documents WHERE text_content='recovered synthetic text'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    pipeline.shutdown().await;
}

#[cfg(target_os = "linux")]
async fn mixed_source_namespaces(pool: &PgPool) {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let objects = Arc::new(InMemory::new());
    let mut script = tempfile::NamedTempFile::new().unwrap();
    writeln!(script, "#!/bin/sh\ncat >/dev/null\ncat \"$1\"").unwrap();
    script
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .unwrap();
    let script = script.into_temp_path();
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects.clone(),
        Extractor::new(
            script.to_path_buf(),
            ExtractionLimits::default(),
            Duration::from_secs(5),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    let version = PreparedDocument::new("hello", json!({}), SourceProfile::OcrVerbatimV1, None)
        .unwrap()
        .signature_version()
        .to_owned();
    sqlx::query("INSERT INTO signature_versions(version,configuration) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(&version).bind(version.parse::<SignatureVersion>().unwrap().configuration()).execute(pool).await.unwrap();
    let native = seed(pool,&objects,"ingest",&serde_json::to_vec(&json!({"Ok":{"file_type":"docx","text":"unique mixed source words","metadata":{},"ocr":false}})).unwrap()).await;
    let mut ocr = seed(pool,&objects,"ingest",&serde_json::to_vec(&json!({"Ok":{"file_type":"png","text":"unique mixed source words","metadata":{},"ocr":true}})).unwrap()).await;
    sqlx::query("UPDATE batch_files SET batch_id=$2,ordinal=1 WHERE id=$1")
        .bind(ocr.file_id)
        .bind(native.batch_id)
        .execute(pool)
        .await
        .unwrap();
    ocr.batch_id = native.batch_id;
    pipeline.process(&native, false).await.unwrap();
    pipeline.process(&ocr, false).await.unwrap();
    assert_eq!(state(pool, &native).await, "completed");
    assert_eq!(state(pool, &ocr).await, "completed");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM match_results WHERE file_id=$1")
            .bind(ocr.file_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM batches WHERE id=$1")
            .bind(native.batch_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        "completed"
    );
    pipeline.shutdown().await;
}

#[cfg(target_os = "linux")]
async fn queue_shutdown_and_live_reclaim_protection(pool: &PgPool) {
    use graphile_worker::{WorkerOptions, WorkerUtils};
    use std::{io::Write, os::unix::fs::PermissionsExt};
    WorkerUtils::new(pool.clone(), "whorl_queue")
        .migrate()
        .await
        .unwrap();
    let objects = Arc::new(InMemory::new());
    let mut script = tempfile::NamedTempFile::new().unwrap();
    writeln!(script,"#!/bin/sh\ncat >/dev/null\nsleep 2\nprintf '%s' '{{\"Ok\":{{\"file_type\":\"docx\",\"text\":\"queue shutdown integration\",\"metadata\":{{}},\"ocr\":false}}}}'").unwrap();
    script
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .unwrap();
    let script = script.into_temp_path();
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects.clone(),
        Extractor::new(
            script.to_path_buf(),
            ExtractionLimits::default(),
            Duration::from_secs(10),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    let payload = seed(pool, &objects, "ingest", b"synthetic").await;
    let mut tx = pool.begin().await.unwrap();
    whorl::queue::JobQueue::new(pool.clone(), std::num::NonZeroU16::new(3).unwrap())
        .enqueue(&mut tx, "worker", payload.file_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let build = |signal: tokio::sync::oneshot::Receiver<()>| {
        WorkerOptions::default()
            .pg_pool(pool.clone())
            .schema("whorl_queue")
            .concurrency(1)
            .poll_interval(Duration::from_millis(30))
            .heartbeat_interval(Duration::from_millis(50))
            .sweep_interval(Duration::from_millis(50))
            .sweep_threshold(Duration::from_millis(300))
            .recovery_delay(Duration::from_millis(50))
            .shutdown_interrupted_job_retry_delay(Duration::from_millis(50))
            .shutdown_grace_period(Duration::from_millis(50))
            .listen_os_shutdown_signals(false)
            .shutdown_signal(async move {
                let _ = signal.await;
            })
            .add_extension(pipeline.clone())
            .define_job::<FileJobPayload>()
            .init()
    };
    let (stop, signal) = tokio::sync::oneshot::channel();
    let worker = build(signal).await.unwrap();
    let running = tokio::spawn(async move { worker.run().await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while state(pool, &payload).await != "running" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    running.await.unwrap().unwrap();
    assert_eq!(state(pool, &payload).await, "running");
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (stop1, signal1) = tokio::sync::oneshot::channel();
    let (stop2, signal2) = tokio::sync::oneshot::channel();
    let first = build(signal1).await.unwrap();
    let second = build(signal2).await.unwrap();
    let first = tokio::spawn(async move { first.run().await });
    let second = tokio::spawn(async move { second.run().await });
    tokio::time::timeout(Duration::from_secs(10), async {
        while state(pool, &payload).await != "completed" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    stop1.send(()).unwrap();
    stop2.send(()).unwrap();
    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT attempt FROM batch_files WHERE id=$1")
            .bind(payload.file_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        2
    );
    pipeline.shutdown().await;
}

#[cfg(target_os = "linux")]
async fn run_child_worker(pool: &PgPool, file: Uuid) {
    let objects = Arc::new(InMemory::new());
    let key: String = sqlx::query_scalar("SELECT source_object_key FROM batch_files WHERE id=$1")
        .bind(file)
        .fetch_one(pool)
        .await
        .unwrap();
    objects
        .put(&Path::from(key), b"synthetic".to_vec().into())
        .await
        .unwrap();
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects,
        Extractor::new(
            std::env::var_os("WHORL_WORKER_CHILD_SCRIPT")
                .unwrap()
                .into(),
            ExtractionLimits::default(),
            Duration::from_secs(10),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    graphile_worker::WorkerOptions::default()
        .pg_pool(pool.clone())
        .schema("whorl_queue")
        .concurrency(1)
        .heartbeat_interval(Duration::from_millis(50))
        .sweep_interval(Duration::from_millis(50))
        .sweep_threshold(Duration::from_millis(300))
        .recovery_delay(Duration::from_millis(50))
        .add_extension(pipeline)
        .define_job::<FileJobPayload>()
        .init()
        .await
        .unwrap()
        .run()
        .await
        .unwrap();
}

#[cfg(target_os = "linux")]
async fn killed_worker_recovers(pool: &PgPool) {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let objects = Arc::new(InMemory::new());
    let mut script = tempfile::NamedTempFile::new().unwrap();
    writeln!(script,"#!/bin/sh\ncat >/dev/null\nsleep 2\nprintf '%s' '{{\"Ok\":{{\"file_type\":\"docx\",\"text\":\"killed worker recovery\",\"metadata\":{{}},\"ocr\":false}}}}'").unwrap();
    script
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .unwrap();
    let script = script.into_temp_path();
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects.clone(),
        Extractor::new(
            script.to_path_buf(),
            ExtractionLimits::default(),
            Duration::from_secs(10),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    let payload = seed(pool, &objects, "ingest", b"synthetic").await;
    let mut tx = pool.begin().await.unwrap();
    whorl::queue::JobQueue::new(pool.clone(), std::num::NonZeroU16::new(3).unwrap())
        .enqueue(&mut tx, "worker", payload.file_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut child = tokio::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "durable_processing_and_recovery"])
        .env("WHORL_WORKER_CHILD_FILE", payload.file_id.to_string())
        .env("WHORL_WORKER_CHILD_SCRIPT", &script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while state(pool, &payload).await != "running" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    child.kill().await.unwrap();
    assert!(!child.wait().await.unwrap().success());
    let (stop, signal) = tokio::sync::oneshot::channel();
    let worker = graphile_worker::WorkerOptions::default()
        .pg_pool(pool.clone())
        .schema("whorl_queue")
        .concurrency(1)
        .heartbeat_interval(Duration::from_millis(50))
        .sweep_interval(Duration::from_millis(50))
        .sweep_threshold(Duration::from_millis(300))
        .recovery_delay(Duration::from_millis(50))
        .listen_os_shutdown_signals(false)
        .shutdown_signal(async {
            let _ = signal.await;
        })
        .add_extension(pipeline.clone())
        .define_job::<FileJobPayload>()
        .init()
        .await
        .unwrap();
    let running = tokio::spawn(async move { worker.run().await });
    tokio::time::timeout(Duration::from_secs(10), async {
        while state(pool, &payload).await != "completed" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    stop.send(()).unwrap();
    running.await.unwrap().unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT attempt FROM batch_files WHERE id=$1")
            .bind(payload.file_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM documents WHERE text_content='killed worker recovery'"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        1
    );
    pipeline.shutdown().await;
}

#[cfg(target_os = "linux")]
async fn handler_concurrency_is_bounded(pool: &PgPool) {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    let objects = Arc::new(InMemory::new());
    let mut script = tempfile::NamedTempFile::new().unwrap();
    writeln!(script,"#!/bin/sh\ncat >/dev/null\nsleep 0.3\nprintf '%s' '{{\"Ok\":{{\"file_type\":\"docx\",\"text\":\"bounded handler concurrency\",\"metadata\":{{}},\"ocr\":false}}}}'").unwrap();
    script
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .unwrap();
    let script = script.into_temp_path();
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects.clone(),
        Extractor::new(
            script.to_path_buf(),
            ExtractionLimits::default(),
            Duration::from_secs(10),
        )
        .unwrap(),
        WorkerConfig {
            workers: NonZeroUsize::new(2).unwrap(),
            ..config()
        },
    )
    .unwrap();
    let mut files = Vec::new();
    for _ in 0..3 {
        let payload = seed(pool, &objects, "check", b"synthetic").await;
        let mut tx = pool.begin().await.unwrap();
        whorl::queue::JobQueue::new(pool.clone(), std::num::NonZeroU16::new(3).unwrap())
            .enqueue(&mut tx, "worker", payload.file_id)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        files.push(payload.file_id);
    }
    let (stop, signal) = tokio::sync::oneshot::channel();
    let worker = graphile_worker::WorkerOptions::default()
        .pg_pool(pool.clone())
        .schema("whorl_queue")
        .concurrency(pipeline.concurrency())
        .listen_os_shutdown_signals(false)
        .shutdown_signal(async {
            let _ = signal.await;
        })
        .add_extension(pipeline.clone())
        .define_job::<FileJobPayload>()
        .init()
        .await
        .unwrap();
    let running = tokio::spawn(async move { worker.run().await });
    let mut maximum = 0;
    tokio::time::timeout(Duration::from_secs(10),async {
        loop {
            let counts:(i64,i64)=sqlx::query_as("SELECT count(*) FILTER(WHERE state='running'),count(*) FILTER(WHERE state='completed') FROM batch_files WHERE id=ANY($1)").bind(&files).fetch_one(pool).await.unwrap();
            maximum=maximum.max(counts.0); assert!(counts.0<=2);
            if counts.1==3 {break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    stop.send(()).unwrap();
    running.await.unwrap().unwrap();
    assert_eq!(maximum, 2);
    pipeline.shutdown().await;
}

#[cfg(target_os = "linux")]
#[expect(
    clippy::too_many_lines,
    reason = "One scenario verifies maintenance registration, source derivation, and invalid configuration rejection"
)]
async fn maintenance_registered_nondefault_versions(pool: &PgPool) {
    use std::{io::Write, os::unix::fs::PermissionsExt};
    for source in ["native", "ocr"] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_maintenance"))
            .args(["reindex", source, "4"])
            .env(
                "DATABASE_URL",
                std::env::var("WHORL_WORKER_TEST_URL").unwrap(),
            )
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let objects = Arc::new(InMemory::new());
    let mut script = tempfile::NamedTempFile::new().unwrap();
    writeln!(script, "#!/bin/sh\ncat >/dev/null\ncat \"$1\"").unwrap();
    script
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .unwrap();
    let script = script.into_temp_path();
    let pipeline = ProcessFile::new(
        pool.clone(),
        objects.clone(),
        Extractor::new(
            script.to_path_buf(),
            ExtractionLimits::default(),
            Duration::from_secs(5),
        )
        .unwrap(),
        config(),
    )
    .unwrap();
    let snapshot =
        SignatureVersion::new(SourceProfile::NativeTextV1, NonZeroUsize::new(4).unwrap());
    for ocr in [false, true] {
        let bytes = serde_json::to_vec(&json!({"Ok":{"file_type":if ocr {"png"} else {"docx"},"text":"maintenance registered nondefault source regression", "metadata":{},"ocr":ocr}})).unwrap();
        let payload = seed(pool, &objects, "ingest", &bytes).await;
        sqlx::query("UPDATE batches SET signature_version=$2 WHERE id=$1")
            .bind(payload.batch_id)
            .bind(snapshot.as_str())
            .execute(pool)
            .await
            .unwrap();
        pipeline.process(&payload, false).await.unwrap();
        assert_eq!(state(pool, &payload).await, "completed");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM match_results WHERE file_id=$1")
                .bind(payload.file_id)
                .fetch_one(pool)
                .await
                .unwrap(),
            0
        );
        let source = if ocr {
            SourceProfile::OcrVerbatimV1
        } else {
            SourceProfile::NativeTextV1
        };
        let version = SignatureVersion::new(source, NonZeroUsize::new(4).unwrap());
        assert!(sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT FROM signatures s JOIN batch_files f ON f.document_id=s.document_id WHERE f.id=$1 AND s.version=$2)").bind(payload.file_id).bind(version.as_str()).fetch_one(pool).await.unwrap());
    }
    let payload = seed(pool, &objects, "check", &serde_json::to_vec(&json!({"Ok":{"file_type":"docx","text":"maintenance registered nondefault source regression","metadata":{},"ocr":false}})).unwrap()).await;
    sqlx::query("UPDATE batches SET signature_version=$2 WHERE id=$1")
        .bind(payload.batch_id)
        .bind(snapshot.as_str())
        .execute(pool)
        .await
        .unwrap();
    pipeline.process(&payload, false).await.unwrap();
    assert_eq!(state(pool, &payload).await, "completed");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM match_results WHERE file_id=$1")
            .bind(payload.file_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    sqlx::query("UPDATE signature_versions SET configuration=jsonb_set(configuration,'{k}','3') WHERE version=$1").bind(snapshot.as_str()).execute(pool).await.unwrap();
    let invalid = seed(pool, &objects, "check", &bytes_for_invalid_registration()).await;
    sqlx::query("UPDATE batches SET signature_version=$2 WHERE id=$1")
        .bind(invalid.batch_id)
        .bind(snapshot.as_str())
        .execute(pool)
        .await
        .unwrap();
    pipeline.process(&invalid, false).await.unwrap();
    assert_eq!(state(pool, &invalid).await, "failed");
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT error->>'code' FROM batch_files WHERE id=$1")
            .bind(invalid.file_id)
            .fetch_one(pool)
            .await
            .unwrap(),
        "invalid_job"
    );
    pipeline.shutdown().await;
}

#[cfg(target_os = "linux")]
fn bytes_for_invalid_registration() -> Vec<u8> {
    serde_json::to_vec(&json!({"Ok":{"file_type":"docx","text":"invalid registration source input","metadata":{},"ocr":false}})).unwrap()
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PanicJob;

impl graphile_worker::TaskHandler for PanicJob {
    const IDENTIFIER: &'static str = "audit_panic_job";
    async fn run(
        self,
        ctx: graphile_worker::WorkerContext,
    ) -> impl graphile_worker::IntoTaskHandlerResult {
        let cpu = ctx
            .get_ext::<Arc<whorl::cpu_pool::CpuPool>>()
            .ok_or("missing CPU pool")?;
        cpu.run::<()>(
            whorl::cpu_pool::WorkDeadline::new(std::time::Instant::now() + Duration::from_secs(10)),
            |_| panic!("controlled worker panic"),
        )
        .await
        .map_err(|error| error.to_string())
    }
}

#[tokio::test]
#[ignore = "requires fresh PostgreSQL database in WHORL_WORKER_PANIC_TEST_URL"]
async fn cpu_panic_stops_queue_and_preserves_retry() {
    let pool = PgPool::connect(&std::env::var("WHORL_WORKER_PANIC_TEST_URL").unwrap())
        .await
        .unwrap();
    graphile_worker::WorkerUtils::new(pool.clone(), "whorl_queue")
        .migrate()
        .await
        .unwrap();
    let cpu = Arc::new(
        whorl::cpu_pool::CpuPool::new(NonZeroUsize::MIN, Duration::ZERO, Duration::from_secs(1))
            .unwrap(),
    );
    let failed = Arc::clone(&cpu);
    let worker = graphile_worker::WorkerOptions::default()
        .pg_pool(pool.clone())
        .schema("whorl_queue")
        .concurrency(1)
        .poll_interval(Duration::from_millis(20))
        .listen_os_shutdown_signals(false)
        .shutdown_signal(async move {
            failed.failed().await;
        })
        .add_extension(Arc::clone(&cpu))
        .define_job::<PanicJob>()
        .init()
        .await
        .unwrap();
    let job = graphile_worker::WorkerUtils::new(pool.clone(), "whorl_queue")
        .add_raw_job(
            "audit_panic_job",
            PanicJob,
            graphile_worker::JobSpec {
                max_attempts: Some(3),
                ..graphile_worker::JobSpec::default()
            },
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), worker.run())
        .await
        .unwrap()
        .unwrap();
    assert!(cpu.has_failed());
    assert!(matches!(
        cpu.run(
            whorl::cpu_pool::WorkDeadline::new(std::time::Instant::now() + Duration::from_secs(1)),
            |_| 42
        )
        .await,
        Err(whorl::cpu_pool::CpuError::Panicked)
    ));
    let row = sqlx::query(
        "SELECT attempts,locked_at IS NULL AS released FROM whorl_queue._private_jobs WHERE id=$1",
    )
    .bind(*job.id())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.get::<i16, _>("attempts"), 1);
    assert!(row.get::<bool, _>("released"));
    cpu.shutdown().await;
}
