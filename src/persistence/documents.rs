//! Client-scoped, versioned candidate persistence.

use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

use crate::service::matching::PreparedDocument;

pub(crate) const PAGE_SQL: &str = include_str!("../../database/queries/candidates.sql");

pub(crate) struct Candidate {
    pub id: Uuid,
    pub text: String,
    pub metadata: serde_json::Value,
}

impl TryFrom<PgRow> for Candidate {
    type Error = sqlx::Error;

    fn try_from(row: PgRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: row.try_get(0)?,
            text: row.try_get(1)?,
            metadata: row.try_get(2)?,
        })
    }
}

pub(crate) struct CandidateRepository<'a> {
    pub connection: &'a mut PgConnection,
    pub client_id: &'a str,
}

impl CandidateRepository<'_> {
    pub async fn duplicate(
        &mut self,
        input: &PreparedDocument,
    ) -> Result<Option<Candidate>, sqlx::Error> {
        sqlx::query(
            "SELECT d.id, d.text_content, d.metadata FROM documents d
             JOIN signatures s ON s.document_id = d.id
             WHERE d.client_id = $1 AND d.content_hash = $2 AND s.version = $3",
        )
        .bind(self.client_id)
        .bind(&input.hash[..])
        .bind(&input.version)
        .fetch_optional(&mut *self.connection)
        .await?
        .map(TryInto::try_into)
        .transpose()
    }

    pub async fn page(
        &mut self,
        input: &PreparedDocument,
        after: Option<Uuid>,
        weighted: bool,
    ) -> Result<Vec<Candidate>, sqlx::Error> {
        let tokens: Vec<&str> = if weighted {
            input.tokens.iter().collect()
        } else {
            Vec::new()
        };
        sqlx::query(PAGE_SQL)
            .bind(self.client_id)
            .bind(&input.version)
            .bind(after)
            .bind(&input.hash[..])
            .bind(&input.bands)
            .bind(tokens)
            .bind(32_i64)
            .fetch_all(&mut *self.connection)
            .await?
            .into_iter()
            .map(TryInto::try_into)
            .collect()
    }

    pub async fn ingest(
        &mut self,
        id: Uuid,
        input: &PreparedDocument,
        file_type: &str,
    ) -> Result<Uuid, sqlx::Error> {
        let tokens: Vec<&str> = input.tokens.iter().collect();
        let canonical: Uuid = sqlx::query_scalar(
            "INSERT INTO documents (id, client_id, content_hash, text_content, file_type, metadata, metadata_tokens)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (client_id, content_hash) DO UPDATE SET content_hash = documents.content_hash
             RETURNING id")
            .bind(id).bind(self.client_id).bind(&input.hash[..]).bind(&input.text).bind(file_type).bind(&input.metadata).bind(tokens)
            .fetch_one(&mut *self.connection).await?;
        sqlx::query("INSERT INTO signatures (document_id, version, values, band_hashes) VALUES ($1, $2, $3, $4)
             ON CONFLICT (document_id, version) DO NOTHING")
            .bind(canonical).bind(&input.version).bind(&input.values).bind(input.optional_bands())
            .execute(&mut *self.connection).await?;
        Ok(canonical)
    }
}
