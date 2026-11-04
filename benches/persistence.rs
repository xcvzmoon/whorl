use sqlx::{PgPool, Postgres, Transaction};
use std::time::Instant;
use uuid::Uuid;

const INSERT: &str = include_str!("../database/queries/insert_matches.sql");

async fn persist(
    tx: &mut Transaction<'_, Postgres>,
    file: Uuid,
    documents: &[Uuid],
    batch_size: usize,
) -> Result<(), sqlx::Error> {
    if batch_size == 1 {
        for document in documents {
            sqlx::query("INSERT INTO match_results(client_id,file_id,document_id,score,text_similarity,metadata_similarity) VALUES($1,$2,$3,$4,$5,$6)")
                .bind("benchmark").bind(file).bind(document).bind(0.9_f64).bind(0.9_f64).bind(Some(0.5_f64))
                .execute(&mut **tx).await?;
        }
    } else {
        for chunk in documents.chunks(batch_size) {
            let scores = vec![0.9_f64; chunk.len()];
            let metadata = vec![Some(0.5_f64); chunk.len()];
            sqlx::query(INSERT)
                .bind("benchmark")
                .bind(file)
                .bind(chunk)
                .bind(&scores)
                .bind(&scores)
                .bind(&metadata)
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Ok(url) = std::env::var("WHORL_PERSISTENCE_BENCH_URL") else {
        println!(
            "SKIP persistence benchmark: set WHORL_PERSISTENCE_BENCH_URL to a fresh disposable database"
        );
        return Ok(());
    };
    let pool = PgPool::connect(&url).await?;
    sqlx::raw_sql(include_str!("../database/migrations/0001_application.sql"))
        .execute(&pool)
        .await?;
    sqlx::raw_sql("INSERT INTO signature_versions(version,configuration) VALUES('benchmark','{}'); INSERT INTO documents(id,client_id,content_hash,text_content,file_type,metadata) SELECT md5('document-' || n)::uuid,'benchmark',sha256(convert_to('document-' || n,'UTF8')),'content','pdf','{}' FROM generate_series(1,10000) n;")
        .execute(&pool).await?;
    let batch = Uuid::new_v4();
    let file = Uuid::new_v4();
    sqlx::query("INSERT INTO batches(id,client_id,mode,signature_version,policy,deadline_at) VALUES($1,'benchmark','check','benchmark','{}',clock_timestamp()+interval '1 hour')")
        .bind(batch).execute(&pool).await?;
    sqlx::query("INSERT INTO batch_files(id,batch_id,client_id,ordinal,name,deadline_at) VALUES($1,$2,'benchmark',0,'input',clock_timestamp()+interval '1 hour')")
        .bind(file).bind(batch).execute(&pool).await?;
    let documents: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM documents ORDER BY id")
        .fetch_all(&pool)
        .await?;
    println!(
        "PostgreSQL loopback, actual match schema/revision trigger, rollback after every iteration; includes driver and database work, not extraction/matching."
    );
    for count in [32_usize, 1_000, 10_000] {
        for batch_size in [1_usize, 128, 512, 1024] {
            let mut samples = Vec::new();
            for iteration in 0..6 {
                let mut tx = pool.begin().await?;
                let start = Instant::now();
                persist(&mut tx, file, &documents[..count], batch_size).await?;
                let elapsed = start.elapsed().as_micros();
                tx.rollback().await?;
                if iteration > 0 {
                    samples.push(elapsed);
                }
            }
            samples.sort_unstable();
            println!(
                "matches={count} batch_size={batch_size} statements={} median_us={}",
                count.div_ceil(batch_size),
                samples[samples.len() / 2]
            );
        }
    }
    Ok(())
}
