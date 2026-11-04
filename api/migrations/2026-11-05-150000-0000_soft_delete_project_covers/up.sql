-- Removing a project's cover hides it rather than deleting it, so the removal can be undone. The
-- image stays in `cover_image` until a new cover replaces it, so a project keeps at most one
-- removed cover. See docs/api.md, Projects.
ALTER TABLE projects
    -- When the cover was removed; NULL while it shows, and without a cover.
    ADD COLUMN cover_removed_at TIMESTAMPTZ,
    ADD CONSTRAINT projects_cover_removed_has_image
        CHECK (cover_removed_at IS NULL OR cover_image IS NOT NULL);
