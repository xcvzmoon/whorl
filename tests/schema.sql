INSERT INTO signature_versions VALUES ('v1', '{}'), ('v2', '{}');
INSERT INTO documents (id, client_id, content_hash, text_content, file_type, metadata)
VALUES ('10000000-0000-0000-0000-000000000001', 'owner', decode(repeat('ab', 32), 'hex'), 'hello', 'pdf', '{}');
INSERT INTO signatures SELECT '10000000-0000-0000-0000-000000000001', version, array_fill(-1, ARRAY[125]), array_fill(-2147483648, ARRAY[25]) FROM signature_versions;
INSERT INTO batches (id, client_id, mode, signature_version, policy, deadline_at)
VALUES ('20000000-0000-0000-0000-000000000001', 'owner', 'ingest', 'v1', '{}', clock_timestamp() + interval '1 hour');
INSERT INTO batch_files (id, batch_id, client_id, ordinal, name, deadline_at)
VALUES ('30000000-0000-0000-0000-000000000001', '20000000-0000-0000-0000-000000000001', 'owner', 0, 'a.pdf', clock_timestamp() + interval '1 hour');
DO $$
DECLARE before_revision bigint;
BEGIN
    IF (SELECT count(*) FROM signatures) <> 2 THEN RAISE EXCEPTION 'version coexistence'; END IF;
    IF NOT EXISTS (SELECT FROM pg_indexes WHERE indexname = 'documents_metadata' AND indexdef LIKE '%jsonb_path_ops%') THEN RAISE EXCEPTION 'metadata index'; END IF;
    IF NOT EXISTS (SELECT FROM pg_indexes WHERE indexname = 'signatures_bands' AND indexdef LIKE '%gin%') THEN RAISE EXCEPTION 'bands index'; END IF;
    BEGIN
        INSERT INTO documents SELECT '10000000-0000-0000-0000-000000000002', client_id, content_hash, text_content, file_type, metadata, metadata_tokens, created_at FROM documents;
        RAISE EXCEPTION 'duplicate accepted';
    EXCEPTION WHEN unique_violation THEN NULL; END;
    INSERT INTO documents SELECT '10000000-0000-0000-0000-000000000002', 'other', content_hash, text_content, file_type, metadata, metadata_tokens, created_at FROM documents;
    BEGIN
        UPDATE signatures SET values = ARRAY[1];
        RAISE EXCEPTION 'invalid signature accepted';
    EXCEPTION WHEN check_violation THEN NULL; END;
    BEGIN
        UPDATE signatures SET band_hashes = array_fill(NULL::integer, ARRAY[25]);
        RAISE EXCEPTION 'null bands accepted';
    EXCEPTION WHEN check_violation THEN NULL; END;
    BEGIN
        UPDATE batch_files SET state = 'failed';
        RAISE EXCEPTION 'failure without error accepted';
    EXCEPTION WHEN check_violation THEN NULL; END;
    BEGIN
        UPDATE batches SET state = 'completed';
        RAISE EXCEPTION 'terminal without retention accepted';
    EXCEPTION WHEN check_violation THEN NULL; END;
    SELECT revision INTO before_revision FROM batches;
    UPDATE batch_files SET state = 'running', stage = 'extracting', attempt = 1;
    IF (SELECT revision FROM batches) <= before_revision THEN RAISE EXCEPTION 'file revision'; END IF;
    UPDATE batch_files SET state = 'completed', stage = 'completed', document_id = '10000000-0000-0000-0000-000000000001';
    BEGIN
        INSERT INTO match_results VALUES ('owner', '30000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000002', 1, 1, NULL);
        RAISE EXCEPTION 'cross client result accepted';
    EXCEPTION WHEN foreign_key_violation THEN NULL; END;
    SELECT revision INTO before_revision FROM batches;
    INSERT INTO match_results VALUES ('owner', '30000000-0000-0000-0000-000000000001', '10000000-0000-0000-0000-000000000001', 1, 1, NULL);
    IF (SELECT revision FROM batches) <= before_revision THEN RAISE EXCEPTION 'result revision'; END IF;
END;
$$;
INSERT INTO capacity_limits VALUES ('global', 5, 50), ('client:race', 3, 30), ('client:other', 5, 50);
DO $$
BEGIN
    IF reserve_capacity('40000000-0000-0000-0000-000000000001', 'race', 1, 31, clock_timestamp() + interval '1 hour') THEN RAISE EXCEPTION 'client bytes exceeded'; END IF;
    IF reserve_capacity('40000000-0000-0000-0000-000000000001', 'other', 6, 1, clock_timestamp() + interval '1 hour') THEN RAISE EXCEPTION 'global jobs exceeded'; END IF;
    IF reserve_capacity('40000000-0000-0000-0000-000000000001', 'other', 1, 51, clock_timestamp() + interval '1 hour') THEN RAISE EXCEPTION 'global bytes exceeded'; END IF;
    BEGIN
        PERFORM reserve_capacity('40000000-0000-0000-0000-000000000001', 'race', -1, 1, clock_timestamp() + interval '1 hour');
        RAISE EXCEPTION 'negative reservation accepted';
    EXCEPTION WHEN invalid_parameter_value THEN NULL; END;
END;
$$;
BEGIN;
SELECT reserve_capacity('40000000-0000-0000-0000-000000000001', 'race', 3, 30, clock_timestamp() + interval '1 hour');
ROLLBACK;
