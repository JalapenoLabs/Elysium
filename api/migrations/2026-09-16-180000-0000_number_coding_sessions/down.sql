-- A number has no UUID to become, so every session is dropped again. Delete sessions
-- through the API first, as for the up migration.

DELETE FROM coding_sessions;

ALTER TABLE coding_sessions ALTER COLUMN id DROP IDENTITY;

-- The table is empty, so the USING expression is never evaluated.
ALTER TABLE coding_sessions ALTER COLUMN id TYPE UUID USING gen_random_uuid();
