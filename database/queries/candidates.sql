SELECT d.id, d.text_content, d.metadata
FROM documents d
JOIN (
    SELECT s.document_id FROM signatures s JOIN documents owner ON owner.id = s.document_id
    WHERE owner.client_id = $1 AND s.version = $2 AND s.band_hashes && $5::integer[]
    UNION
    SELECT m.id FROM documents m JOIN signatures versioned ON versioned.document_id = m.id
    WHERE m.client_id = $1 AND versioned.version = $2 AND m.metadata_tokens && $6::text[]
) candidates ON candidates.document_id = d.id
WHERE d.client_id = $1 AND ($3::uuid IS NULL OR d.id > $3) AND d.content_hash <> $4
ORDER BY d.id LIMIT $7
