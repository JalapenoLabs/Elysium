-- A cover image per project, shown as the banner on its tile. The API stores it already
-- compressed to WebP, so the column holds what browsers are served, never the upload.

ALTER TABLE projects
    ADD COLUMN cover_image BYTEA,
    -- When the cover last changed; clients add it to the image URL so caches refresh.
    ADD COLUMN cover_image_updated_at TIMESTAMPTZ,
    ADD CONSTRAINT projects_cover_image_updated CHECK ((cover_image IS NULL) = (cover_image_updated_at IS NULL)),
    -- Compressed covers are far smaller; this only bounds what a bug could store.
    ADD CONSTRAINT projects_cover_image_size CHECK (octet_length(cover_image) <= 1000000);
