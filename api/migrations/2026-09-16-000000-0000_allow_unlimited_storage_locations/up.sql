-- A storage location may have no limit: NULL means Elysium stores as much as the provider
-- allows. storage_locations_storage_limit_positive still holds for any limit that is set.
ALTER TABLE storage_locations ALTER COLUMN storage_limit_bytes DROP NOT NULL;
