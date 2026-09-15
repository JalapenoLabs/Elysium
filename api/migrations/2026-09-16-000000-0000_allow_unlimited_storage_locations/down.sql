-- Unlimited locations take the largest limit the API accepts (2^53 - 1 bytes), the closest
-- a required limit comes to none.
UPDATE storage_locations SET storage_limit_bytes = 9007199254740991 WHERE storage_limit_bytes IS NULL;
ALTER TABLE storage_locations ALTER COLUMN storage_limit_bytes SET NOT NULL;
