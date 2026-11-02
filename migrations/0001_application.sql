BEGIN;

CREATE TABLE documents (
    id uuid PRIMARY KEY,
    client_id text NOT NULL CHECK (client_id <> ''),
    content_hash bytea NOT NULL CHECK (octet_length(content_hash) = 32),
    text_content text NOT NULL CHECK (text_content <> ''),
    file_type text NOT NULL CHECK (file_type IN ('pdf', 'docx', 'pptx', 'xlsx', 'png', 'jpeg')),
    metadata jsonb NOT NULL CHECK (jsonb_typeof(metadata) = 'object'),
    metadata_tokens text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (client_id, content_hash),
    UNIQUE (client_id, id)
);
CREATE INDEX documents_file_type ON documents (client_id, file_type);
CREATE INDEX documents_metadata ON documents USING gin (metadata jsonb_path_ops);
CREATE INDEX documents_metadata_tokens ON documents USING gin (metadata_tokens);

CREATE TABLE signature_versions (
    version text PRIMARY KEY CHECK (version <> ''),
    configuration jsonb NOT NULL CHECK (jsonb_typeof(configuration) = 'object'),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE signatures (
    document_id uuid NOT NULL REFERENCES documents ON DELETE CASCADE,
    version text NOT NULL REFERENCES signature_versions,
    values integer[] NOT NULL CHECK (array_ndims(values) = 1 AND array_lower(values, 1) = 1 AND cardinality(values) = 125 AND array_position(values, NULL) IS NULL),
    band_hashes integer[] NOT NULL CHECK (array_ndims(band_hashes) = 1 AND array_lower(band_hashes, 1) = 1 AND cardinality(band_hashes) = 25 AND array_position(band_hashes, NULL) IS NULL),
    PRIMARY KEY (document_id, version)
);
CREATE INDEX signatures_values ON signatures USING gin (values);
CREATE INDEX signatures_bands ON signatures USING gin (band_hashes);
CREATE INDEX signatures_version ON signatures (version, document_id);

CREATE TABLE batches (
    id uuid PRIMARY KEY,
    client_id text NOT NULL CHECK (client_id <> ''),
    mode text NOT NULL CHECK (mode IN ('check', 'ingest')),
    state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'completed', 'partially_failed', 'failed')),
    signature_version text NOT NULL REFERENCES signature_versions,
    policy jsonb NOT NULL CHECK (jsonb_typeof(policy) = 'object'),
    revision bigint NOT NULL DEFAULT 0 CHECK (revision >= 0),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    deadline_at timestamptz NOT NULL,
    completed_at timestamptz,
    expires_at timestamptz,
    tombstone_until timestamptz,
    UNIQUE (client_id, id),
    CHECK (deadline_at > created_at),
    CHECK ((state IN ('completed', 'partially_failed', 'failed')) = (completed_at IS NOT NULL)),
    CHECK ((completed_at IS NULL AND expires_at IS NULL AND tombstone_until IS NULL) OR
           (completed_at IS NOT NULL AND expires_at IS NOT NULL AND tombstone_until IS NOT NULL AND expires_at > completed_at AND tombstone_until > expires_at))
);
CREATE INDEX batches_retention ON batches (expires_at) WHERE expires_at IS NOT NULL;
CREATE INDEX batches_tombstones ON batches (tombstone_until) WHERE tombstone_until IS NOT NULL;

CREATE TABLE batch_files (
    id uuid PRIMARY KEY,
    batch_id uuid NOT NULL,
    client_id text NOT NULL,
    ordinal smallint NOT NULL CHECK (ordinal BETWEEN 0 AND 4),
    name text NOT NULL,
    state text NOT NULL DEFAULT 'queued' CHECK (state IN ('queued', 'running', 'completed', 'failed')),
    stage text CHECK (stage IN ('extracting', 'signing', 'candidate_lookup', 'verifying', 'persisting', 'completed')),
    attempt integer NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    next_retry_at timestamptz,
    deadline_at timestamptz NOT NULL,
    source_object_key text,
    source_bytes bigint NOT NULL DEFAULT 0 CHECK (source_bytes BETWEEN 0 AND 20971520),
    upload_metadata jsonb NOT NULL DEFAULT '{}' CHECK (jsonb_typeof(upload_metadata) = 'object'),
    document_id uuid,
    error jsonb CHECK (jsonb_typeof(error) = 'object' AND error ?& ARRAY['code', 'message', 'retryable']),
    FOREIGN KEY (client_id, batch_id) REFERENCES batches (client_id, id) ON DELETE CASCADE,
    FOREIGN KEY (client_id, document_id) REFERENCES documents (client_id, id),
    UNIQUE (batch_id, ordinal),
    UNIQUE (client_id, id),
    CHECK ((state = 'failed') = (error IS NOT NULL)),
    CHECK (document_id IS NULL OR state = 'completed'),
    CHECK (next_retry_at IS NULL OR (state = 'queued' AND attempt > 0)),
    CHECK ((state = 'running' AND stage IS NOT NULL AND stage <> 'completed') OR
           (state = 'completed' AND stage = 'completed') OR
           (state IN ('queued', 'failed') AND stage IS NULL))
);
CREATE INDEX batch_files_deadline ON batch_files (deadline_at) WHERE state IN ('queued', 'running');
CREATE TABLE match_results (
    client_id text NOT NULL,
    file_id uuid NOT NULL,
    document_id uuid NOT NULL,
    score double precision NOT NULL CHECK (score BETWEEN 0 AND 1),
    text_similarity double precision NOT NULL CHECK (text_similarity BETWEEN 0 AND 1),
    metadata_similarity double precision CHECK (metadata_similarity BETWEEN 0 AND 1),
    FOREIGN KEY (client_id, file_id) REFERENCES batch_files (client_id, id) ON DELETE CASCADE,
    FOREIGN KEY (client_id, document_id) REFERENCES documents (client_id, id),
    PRIMARY KEY (file_id, document_id)
);
CREATE INDEX match_results_order ON match_results (file_id, score DESC, document_id);

CREATE FUNCTION revise_batch() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF TG_TABLE_NAME = 'batch_files' THEN
        UPDATE batches SET revision = revision + 1 WHERE id = COALESCE(NEW.batch_id, OLD.batch_id);
    ELSE
        UPDATE batches SET revision = revision + 1 WHERE id =
            (SELECT batch_id FROM batch_files WHERE id = COALESCE(NEW.file_id, OLD.file_id));
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER batch_files_revision AFTER INSERT OR UPDATE OR DELETE ON batch_files
    FOR EACH ROW EXECUTE FUNCTION revise_batch();
CREATE TRIGGER match_results_revision AFTER INSERT OR UPDATE OR DELETE ON match_results
    FOR EACH ROW EXECUTE FUNCTION revise_batch();
CREATE FUNCTION revise_batch_fields() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.revision := OLD.revision + 1;
    RETURN NEW;
END;
$$;
CREATE TRIGGER batches_revision BEFORE UPDATE ON batches
    FOR EACH ROW EXECUTE FUNCTION revise_batch_fields();

COMMIT;
