-- Postgres cannot drop an enum value, so the kind type is rebuilt without it. The next
-- migration's rollback has already removed every s3 location. The constraint comparing
-- kinds is tied to the old type, so it is dropped and restored around the swap.

DROP TYPE s3_service;

ALTER TABLE storage_locations DROP CONSTRAINT storage_locations_bunny_fields;

ALTER TYPE storage_location_kind RENAME TO storage_location_kind_with_s3;
CREATE TYPE storage_location_kind AS ENUM (
    'bunny'
);
ALTER TABLE storage_locations
    ALTER COLUMN kind TYPE storage_location_kind USING kind::text::storage_location_kind;
DROP TYPE storage_location_kind_with_s3;

ALTER TABLE storage_locations ADD CONSTRAINT storage_locations_bunny_fields CHECK (
    (kind = 'bunny') = (bunny_zone IS NOT NULL AND bunny_region IS NOT NULL)
);
