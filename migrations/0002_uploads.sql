BEGIN;
CREATE TABLE stored_uploads (
    id uuid PRIMARY KEY REFERENCES capacity_reservations(id),
    client_id text NOT NULL CHECK (client_id <> ''),
    object_key text NOT NULL UNIQUE,
    multipart_id text,
    bytes bigint CHECK (bytes BETWEEN 1 AND 20971520),
    expires_at timestamptz NOT NULL
);
CREATE INDEX stored_uploads_expiry ON stored_uploads(expires_at);
COMMIT;
