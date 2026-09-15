-- S3-compatible buckets as a storage kind. A new enum value cannot be used in the
-- transaction that adds it, so the columns that refer to it follow in the next migration.

ALTER TYPE storage_location_kind ADD VALUE 's3';

-- The services Elysium reaches over the S3 API. Each has a fixed endpoint in code.
CREATE TYPE s3_service AS ENUM (
    'aws',
    'google_cloud'
);
