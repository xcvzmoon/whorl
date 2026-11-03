use serde_json::{Value, json};
use sqlx::{PgPool as Client, Row};
use std::sync::Arc;
use uuid::Uuid;
use whorl::{
    service::matching::{Matcher, MatchingError, PreparedDocument},
    similarity::{
        normalize::NormalizationVersion,
        shingles::{ShingleSet, SourceProfile},
        signature::MinHashSignature,
        verify::SimilarityPolicy,
    },
};

const VERSION: &str = "nfkc-v1/native-text-v1/word-3/short-equality-v1/xxh3-low32-le-v1/125/25x5";

async fn connect() -> Client {
    let url = std::env::var("WHORL_CANDIDATES_TEST_URL")
        .expect("set WHORL_CANDIDATES_TEST_URL to a fresh disposable PostgreSQL database");
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap()
}

fn prepared(text: &str, metadata: Value) -> Arc<PreparedDocument> {
    Arc::new(PreparedDocument::new(text, metadata, SourceProfile::NativeTextV1, None).unwrap())
}

async fn upload(
    client: &Client,
    id: u128,
    owner: &str,
    mode: &str,
    version: &str,
    metadata: Value,
) -> Uuid {
    let batch = Uuid::from_u128(id + 10_000);
    let file = Uuid::from_u128(id + 20_000);
    sqlx::query("INSERT INTO batches (id, client_id, mode, signature_version, policy, deadline_at) VALUES ($1,$2,$3,$4,'{}',clock_timestamp()+interval '1 hour')").bind(batch).bind(owner).bind(mode).bind(version).execute(client).await.unwrap();
    sqlx::query("INSERT INTO batch_files (id,batch_id,client_id,ordinal,name,deadline_at,upload_metadata,source_object_key) VALUES ($1,$2,$3,0,'upload.pdf',clock_timestamp()+interval '1 hour',$4,'source-key')").bind(file).bind(batch).bind(owner).bind(&metadata).execute(client).await.unwrap();
    file
}

async fn ingest(client: &Client, id: u128, owner: &str, text: &str, metadata: Value) -> Uuid {
    let file = upload(client, id, owner, "ingest", VERSION, metadata.clone()).await;
    prepared(text, metadata)
        .ingest(client, owner, Uuid::from_u128(id), file, "pdf")
        .await
        .unwrap()
}

#[test]
fn rejects_empty_text_and_nonobject_metadata() {
    assert!(matches!(
        PreparedDocument::new("!!!", json!({}), SourceProfile::NativeTextV1, None),
        Err(MatchingError::EmptyText(_))
    ));
    assert!(matches!(
        PreparedDocument::new("hello", json!([]), SourceProfile::NativeTextV1, None),
        Err(MatchingError::InvalidMetadata)
    ));
}

#[tokio::test]
#[ignore = "requires a fresh disposable PostgreSQL database; see docs/persistence.md"]
async fn postgres_candidate_matching_and_duplicate_races() {
    let pool = connect().await;
    let client = &pool;
    sqlx::raw_sql(include_str!("../migrations/0001_application.sql"))
        .execute(client)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO signature_versions(version,configuration) VALUES ($1,'{}'),('other','{}')",
    )
    .bind(VERSION)
    .execute(client)
    .await
    .unwrap();
    let matcher = Matcher::new(std::sync::Arc::new(
        whorl::cpu_pool::CpuPool::new(
            std::num::NonZeroUsize::new(1).unwrap(),
            std::time::Duration::ZERO,
            std::time::Duration::from_millis(10),
        )
        .unwrap(),
    ));
    let text = "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty";
    let canonical = ingest(client, 1, "owner", text, json!({"author":"first"})).await;
    let duplicate_file = upload(
        client,
        2,
        "owner",
        "ingest",
        VERSION,
        json!({"author":"second"}),
    )
    .await;
    assert_eq!(
        prepared(text, json!({"author":"second"}))
            .ingest(&pool, "owner", Uuid::from_u128(2), duplicate_file, "docx")
            .await
            .unwrap(),
        canonical
    );
    let row = sqlx::query("SELECT metadata,file_type,(SELECT upload_metadata FROM batch_files WHERE id=$1),(SELECT source_object_key FROM batch_files WHERE id=$1) FROM documents WHERE id=$2").bind(duplicate_file).bind(canonical).fetch_one(client).await.unwrap();
    assert_eq!(row.get::<Value, _>(0), json!({"author":"first"}));
    assert_eq!(row.get::<String, _>(1), "pdf");
    assert_eq!(row.get::<Value, _>(2), json!({"author":"second"}));
    assert_eq!(row.get::<String, _>(3), "source-key");

    let near = ingest(
        &pool,
        3,
        "owner",
        &format!("{text} extra"),
        json!({"author":"first"}),
    )
    .await;
    let foreign = ingest(client, 4, "foreign", text, json!({"author":"first"})).await;
    let isolated = ingest(
        &pool,
        5,
        "owner",
        &format!("{text} isolated"),
        json!({"author":"first"}),
    )
    .await;
    sqlx::query("UPDATE signatures SET version='other' WHERE document_id=$1")
        .bind(isolated)
        .execute(client)
        .await
        .unwrap();
    let false_positive = ingest(&pool, 6, "owner", "entirely unrelated words", json!({})).await;
    sqlx::query("UPDATE signatures SET band_hashes=(SELECT band_hashes FROM signatures WHERE document_id=$1) WHERE document_id=$2").bind(canonical).bind(false_positive).execute(client).await.unwrap();
    let snapshot_sql = "SELECT jsonb_build_array(
        (SELECT jsonb_agg(to_jsonb(d) ORDER BY id) FROM documents d),
        (SELECT jsonb_agg(to_jsonb(s) ORDER BY document_id, version) FROM signatures s),
        (SELECT jsonb_agg(to_jsonb(f) ORDER BY id) FROM batch_files f),
        (SELECT jsonb_agg(to_jsonb(b) ORDER BY id) FROM batches b))";
    let before: Value = sqlx::query(snapshot_sql)
        .fetch_one(client)
        .await
        .unwrap()
        .get(0);
    let results = matcher
        .check(
            &pool,
            "owner",
            prepared(text, json!({"author":"second"})),
            SimilarityPolicy::default(),
            whorl::cpu_pool::WorkDeadline::new(
                std::time::Instant::now() + std::time::Duration::from_secs(60),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        results.iter().map(|m| m.document_id).collect::<Vec<_>>(),
        vec![canonical, near]
    );
    assert_eq!(results[0].score.text_similarity, 1.0);
    assert_eq!(results[0].score.metadata_similarity, Some(0.0));
    assert!(
        !results
            .iter()
            .any(|m| [foreign, isolated, false_positive].contains(&m.document_id))
    );
    let after: Value = sqlx::query(snapshot_sql)
        .fetch_one(client)
        .await
        .unwrap()
        .get(0);
    assert_eq!(before, after);

    let weighted_union = matcher
        .check(
            &pool,
            "owner",
            prepared(text, json!({"author":"first"})),
            SimilarityPolicy::new(0.75, Some(0.5)).unwrap(),
            whorl::cpu_pool::WorkDeadline::new(
                std::time::Instant::now() + std::time::Duration::from_secs(60),
            ),
        )
        .await
        .unwrap();
    assert_eq!(
        weighted_union
            .iter()
            .map(|m| m.document_id)
            .collect::<Vec<_>>(),
        vec![canonical, near]
    );

    let short = ingest(client, 7, "owner", "tiny", json!({"author":"first"})).await;
    let exact = matcher
        .check(
            &pool,
            "owner",
            prepared("TINY!", json!({})),
            SimilarityPolicy::default(),
            whorl::cpu_pool::WorkDeadline::new(
                std::time::Instant::now() + std::time::Duration::from_secs(60),
            ),
        )
        .await
        .unwrap();
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].document_id, short);
    for id in 100..170 {
        ingest(
            &pool,
            id,
            "owner",
            &format!("unrelated{id}"),
            json!({"group":"shared"}),
        )
        .await;
    }
    let weighted = SimilarityPolicy::new(0.75, Some(0.0)).unwrap();
    let results = matcher
        .check(
            &pool,
            "owner",
            prepared("query", json!({"group":"shared"})),
            weighted,
            whorl::cpu_pool::WorkDeadline::new(
                std::time::Instant::now() + std::time::Duration::from_secs(60),
            ),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 70);
    assert!(
        results
            .windows(2)
            .all(|pair| pair[0].document_id < pair[1].document_id)
    );
    let text_only = matcher
        .check(
            &pool,
            "owner",
            prepared("query", json!({"group":"shared"})),
            SimilarityPolicy::default(),
            whorl::cpu_pool::WorkDeadline::new(
                std::time::Instant::now() + std::time::Duration::from_secs(60),
            ),
        )
        .await
        .unwrap();
    assert!(text_only.is_empty());

    let check_file = upload(client, 8, "owner", "check", VERSION, json!({})).await;
    assert!(
        prepared("check only", json!({}))
            .ingest(client, "owner", Uuid::from_u128(8), check_file, "pdf")
            .await
            .is_err()
    );
    assert!(
        prepared("other owner", json!({}))
            .ingest(&pool, "foreign", Uuid::from_u128(8), duplicate_file, "pdf")
            .await
            .is_err()
    );

    let mut tasks = Vec::new();
    for id in 200..208 {
        let file = upload(client, id, "owner", "ingest", VERSION, json!({"race":id})).await;
        let pool = pool.clone();
        tasks.push(tokio::spawn(async move {
            prepared("race content", json!({"race":id}))
                .ingest(&pool, "owner", Uuid::from_u128(id), file, "pdf")
                .await
                .unwrap()
        }));
    }
    let mut ids = Vec::new();
    for task in tasks {
        ids.push(task.await.unwrap());
    }
    assert!(ids.iter().all(|id| *id == ids[0]));
    let rows = sqlx::query("SELECT document_id,upload_metadata FROM batch_files WHERE id >= $1 AND id < $2 ORDER BY id").bind(Uuid::from_u128(20200)).bind(Uuid::from_u128(20208)).fetch_all(client).await.unwrap();
    assert_eq!(rows.len(), 8);
    for (offset, row) in rows.iter().enumerate() {
        assert_eq!(row.get::<Uuid, _>(0), ids[0]);
        assert_eq!(row.get::<Value, _>(1), json!({"race":200+offset}));
    }

    sqlx::raw_sql("INSERT INTO documents (id,client_id,content_hash,text_content,file_type,metadata,metadata_tokens)
        SELECT md5('plan' || n)::uuid,'owner',sha256(convert_to('plan' || n,'UTF8')),'plan filler ' || n,'pdf','{}',ARRAY['filler'] FROM generate_series(1,10000) n;").execute(client).await.unwrap();
    sqlx::query("INSERT INTO signatures SELECT id,$1,array_fill(0,ARRAY[125]),array_fill(0,ARRAY[25]) FROM documents WHERE metadata_tokens = ARRAY['filler']").bind(VERSION).execute(client).await.unwrap();
    sqlx::raw_sql("ANALYZE documents; ANALYZE signatures")
        .execute(client)
        .await
        .unwrap();
    let signature = MinHashSignature::new(
        &ShingleSet::new(
            text,
            NormalizationVersion::V1,
            SourceProfile::NativeTextV1,
            None,
        )
        .unwrap(),
    )
    .unwrap();
    let bands: Vec<i32> = signature
        .bands()
        .values()
        .iter()
        .map(|v| i32::from_ne_bytes(v.to_ne_bytes()))
        .collect();
    let tokens = vec!["/author=\"first\""];
    let statement = concat!(
        "EXPLAIN ",
        include_str!("../src/persistence/candidates.sql")
    );
    let plan = sqlx::query(statement)
        .bind("owner")
        .bind(VERSION)
        .bind(None::<Uuid>)
        .bind(&[0_u8; 32][..])
        .bind(&bands)
        .bind(&tokens)
        .bind(32_i64)
        .fetch_all(client)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.get::<String, _>(0))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(plan.contains("signatures_bands"), "{plan}");
    assert!(plan.contains("documents_metadata_tokens"), "{plan}");
    eprintln!("{plan}");
    pool.close().await;
}
