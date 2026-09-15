-- S3 locations cannot exist without these columns, so rolling back deletes them.
DELETE FROM storage_locations WHERE kind::text = 's3';

ALTER TABLE storage_locations
    DROP COLUMN s3_access_key_id,
    DROP COLUMN s3_region,
    DROP COLUMN s3_bucket,
    DROP COLUMN s3_service;
