#[path = "../src/persistence/results.rs"]
mod results;

use sqlx::{PgPool, Row};
use uuid::Uuid;
use whorl::{queue, service, similarity::verify::MatchScore};

async fn file(pool: &PgPool, owner: &str) -> queue::FileJobPayload {
    let batch_id = Uuid::new_v4();
    let file_id = Uuid::new_v4();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES($1,$2,'check','test','{}',clock_timestamp()+interval '1 hour')")
        .bind(batch_id).bind(owner).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at) VALUES($1,$2,$3,0,'input',clock_timestamp()+interval '1 hour')")
        .bind(file_id).bind(batch_id).bind(owner).execute(pool).await.unwrap();
    queue::FileJobPayload {
        client_id: owner.into(),
        batch_id,
        file_id,
    }
}

fn found(document_id: Uuid, n: usize) -> service::matching::DocumentMatch {
    service::matching::DocumentMatch {
        document_id,
        score: MatchScore {
            score: 0.123_456_789_012_345_6,
            text_similarity: 0.987_654_321_098_765_4,
            metadata_similarity: n.is_multiple_of(2).then_some(0.314_159_265_358_979_3),
            qualifies: true,
        },
    }
}

async fn count(pool: &PgPool, file: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM match_results WHERE file_id=$1")
        .bind(file)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "One disposable schema verifies multi-chunk commit and rollback outcomes together"
)]
#[ignore = "requires fresh WHORL_MATCH_PERSISTENCE_TEST_URL PostgreSQL database"]
#[allow(
    clippy::float_cmp,
    reason = "Persisted f64 scores must round-trip without changing their exact value"
)]
async fn bounded_results_preserve_scores_scope_and_atomicity() {
    let pool = PgPool::connect(&std::env::var("WHORL_MATCH_PERSISTENCE_TEST_URL").unwrap())
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("../database/migrations/0001_application.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("INSERT INTO signature_versions(version,configuration) VALUES('test','{}'); INSERT INTO documents(id,client_id,content_hash,text_content,file_type,metadata) SELECT md5('document-' || n)::uuid,'owner',sha256(convert_to('document-' || n,'UTF8')),'content','pdf','{}' FROM generate_series(1,1030) n;")
        .execute(&pool).await.unwrap();
    let documents: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM documents ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    let matches: Vec<_> = documents
        .iter()
        .enumerate()
        .map(|(n, &id)| found(id, n))
        .collect();
    let payload = file(&pool, "owner").await;
    let mut tx = pool.begin().await.unwrap();
    results::persist_matches(&mut tx, &payload, &[])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(count(&pool, payload.file_id).await, 0);
    let mut tx = pool.begin().await.unwrap();
    results::persist_matches(&mut tx, &payload, &matches)
        .await
        .unwrap();
    assert_eq!(count(&pool, payload.file_id).await, 0);
    tx.commit().await.unwrap();
    let rows = sqlx::query("SELECT document_id,score,text_similarity,metadata_similarity FROM match_results WHERE file_id=$1 ORDER BY document_id")
        .bind(payload.file_id).fetch_all(&pool).await.unwrap();
    assert_eq!(rows.len(), matches.len());
    for (row, expected) in rows.iter().zip(&matches) {
        assert_eq!(row.get::<Uuid, _>("document_id"), expected.document_id);
        assert_eq!(row.get::<f64, _>("score"), expected.score.score);
        assert_eq!(
            row.get::<f64, _>("text_similarity"),
            expected.score.text_similarity
        );
        assert_eq!(
            row.get::<Option<f64>, _>("metadata_similarity"),
            expected.score.metadata_similarity
        );
    }
    let mut tx = pool.begin().await.unwrap();
    assert!(
        results::persist_matches(&mut tx, &payload, &matches)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool, payload.file_id).await, 1030);

    let second = file(&pool, "owner").await;
    let mut tx = pool.begin().await.unwrap();
    results::persist_matches(&mut tx, &second, &matches)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool, second.file_id).await, 0);

    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO documents(id,client_id,content_hash,text_content,file_type,metadata) VALUES($1,'foreign',sha256('foreign'::bytea),'foreign','pdf','{}')")
        .bind(foreign).execute(&pool).await.unwrap();
    let mut invalid: Vec<_> = documents[..1024]
        .iter()
        .enumerate()
        .map(|(n, &id)| found(id, n))
        .collect();
    invalid.push(found(foreign, 0));
    let mut tx = pool.begin().await.unwrap();
    let error = results::persist_matches(&mut tx, &second, &invalid)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23503")
    );
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool, second.file_id).await, 0);
    assert_eq!(count(&pool, payload.file_id).await, 1030);

    let foreign_file = file(&pool, "foreign").await;
    let mut tx = pool.begin().await.unwrap();
    let forged = queue::FileJobPayload {
        client_id: "owner".into(),
        ..foreign_file.clone()
    };
    assert!(
        results::persist_matches(&mut tx, &forged, &matches[..1])
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool, foreign_file.file_id).await, 0);

    let mut duplicate: Vec<_> = documents[..1024]
        .iter()
        .enumerate()
        .map(|(n, &id)| found(id, n))
        .collect();
    duplicate.push(found(documents[0], 0));
    let mut tx = pool.begin().await.unwrap();
    let error = results::persist_matches(&mut tx, &second, &duplicate)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23505")
    );
    tx.rollback().await.unwrap();
    assert_eq!(count(&pool, second.file_id).await, 0);
}
