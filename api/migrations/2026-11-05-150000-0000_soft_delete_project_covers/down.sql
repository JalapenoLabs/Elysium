-- Without the column, a hidden cover would show again, so removed covers are deleted first.
UPDATE projects
    SET cover_image = NULL, cover_image_updated_at = NULL
    WHERE cover_removed_at IS NOT NULL;

ALTER TABLE projects
    DROP CONSTRAINT projects_cover_removed_has_image,
    DROP COLUMN cover_removed_at;
