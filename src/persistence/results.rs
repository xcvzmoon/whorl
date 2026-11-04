//! Bounded insertion of verified matches inside the worker's final transaction.

use crate::{queue::FileJobPayload, service::matching::DocumentMatch};
use sqlx::{Postgres, Transaction};

/// Limits each temporary typed array to 512 values per statement.
///
/// Each result contributes a UUID and three f64 score columns, one nullable.
/// Batching amortizes database round trips without changing transaction ownership
/// or the result schema. This is a memory/round-trip tradeoff, not a result cap.
const MATCHES_PER_INSERT: usize = 512;
const INSERT_MATCHES: &str = include_str!("../../database/queries/insert_matches.sql");

/// Writes every verified result without committing the caller's transaction.
///
/// The caller must validate file ownership, current attempt, and deadline before
/// writing. A failed chunk requires rolling back the whole transaction, including
/// earlier chunks. Empty input performs no statements. Scores retain their f64
/// values, absent metadata scores remain SQL NULL, and conflicts are errors.
pub(crate) async fn persist_matches(
    transaction: &mut Transaction<'_, Postgres>,
    payload: &FileJobPayload,
    matches: &[DocumentMatch],
) -> Result<(), sqlx::Error> {
    let capacity = matches.len().min(MATCHES_PER_INSERT);
    let mut documents = Vec::with_capacity(capacity);
    let mut scores = Vec::with_capacity(capacity);
    let mut text_scores = Vec::with_capacity(capacity);
    let mut metadata_scores = Vec::with_capacity(capacity);
    for chunk in matches.chunks(MATCHES_PER_INSERT) {
        documents.clear();
        scores.clear();
        text_scores.clear();
        metadata_scores.clear();
        for found in chunk {
            documents.push(found.document_id);
            scores.push(found.score.score);
            text_scores.push(found.score.text_similarity);
            metadata_scores.push(found.score.metadata_similarity);
        }
        sqlx::query(INSERT_MATCHES)
            .bind(&payload.client_id)
            .bind(payload.file_id)
            .bind(&documents)
            .bind(&scores)
            .bind(&text_scores)
            .bind(&metadata_scores)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}
