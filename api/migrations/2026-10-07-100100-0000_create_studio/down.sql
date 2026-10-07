DROP INDEX storage_locations_one_studio_default_idx;
ALTER TABLE storage_locations DROP COLUMN is_studio_default;

-- Studio sessions have nothing to belong to without their items.
DELETE FROM coding_sessions WHERE studio_item_id IS NOT NULL;
ALTER TABLE coding_sessions DROP CONSTRAINT coding_sessions_project_required;
ALTER TABLE coding_sessions ALTER COLUMN project_id SET NOT NULL;
ALTER TABLE coding_sessions DROP COLUMN studio_item_id;

DROP TABLE studio_feedback;
ALTER TABLE studio_items DROP CONSTRAINT studio_items_thumbnail_asset_id_fkey;
DROP TABLE studio_assets;
DROP TYPE studio_asset_kind;
DROP TABLE studio_items;
