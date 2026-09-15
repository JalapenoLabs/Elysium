ALTER TABLE projects
    DROP CONSTRAINT projects_cover_image_size,
    DROP CONSTRAINT projects_cover_image_updated,
    DROP COLUMN cover_image_updated_at,
    DROP COLUMN cover_image;
