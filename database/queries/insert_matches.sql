INSERT INTO match_results (
    client_id, file_id, document_id, score, text_similarity, metadata_similarity
)
SELECT $1, $2, document_id, score, text_similarity, metadata_similarity
FROM unnest(
    $3::uuid[], $4::double precision[], $5::double precision[], $6::double precision[]
) AS matches(document_id, score, text_similarity, metadata_similarity)
