-- An S3 location's bucket. The secret access key is sealed in access_key_encrypted, like
-- Bunny's password; the access key id names the key and is not secret.
--
-- Kinds are compared as text: a runner that applies this in the same transaction as the
-- migration adding 's3' cannot use the new value as an enum literal yet.

ALTER TABLE storage_locations
    ADD COLUMN s3_service s3_service,
    ADD COLUMN s3_bucket TEXT,
    -- The bucket's region for aws; google_cloud buckets need none.
    ADD COLUMN s3_region TEXT,
    ADD COLUMN s3_access_key_id TEXT,
    ADD CONSTRAINT storage_locations_s3_fields CHECK (
        (kind::text = 's3') = (s3_service IS NOT NULL AND s3_bucket IS NOT NULL AND s3_access_key_id IS NOT NULL)
    ),
    ADD CONSTRAINT storage_locations_s3_region CHECK (
        (s3_service = 'aws') = (s3_region IS NOT NULL)
    ),
    -- AWS and Google Cloud both allow lowercase letters, digits, dots, and hyphens; Google
    -- Cloud also allows underscores and longer dotted names.
    ADD CONSTRAINT storage_locations_s3_bucket_shape CHECK (s3_bucket ~ '^[a-z0-9][a-z0-9._-]{1,220}[a-z0-9]$'),
    ADD CONSTRAINT storage_locations_s3_region_shape CHECK (s3_region ~ '^[a-z0-9-]{1,64}$'),
    -- Postgres caps regex repetition counts at 255, so the length is checked on its own.
    ADD CONSTRAINT storage_locations_s3_access_key_id_shape CHECK (
        s3_access_key_id ~ '^[A-Za-z0-9]+$' AND char_length(s3_access_key_id) <= 256
    );
