-- Files attached to knowledge base pages (`docs/knowledge.md`, Pages, Attachments).
--
-- Neither foreign key cascades, on purpose. A row is the only record of a file kept with a provider,
-- so a page cannot be deleted for good, nor a location forgotten, while a row would be lost with it:
-- whatever deletes either has to deal with the files first.
CREATE TABLE page_attachments (
    id UUID PRIMARY KEY,
    page_id UUID NOT NULL REFERENCES pages (id),
    -- Where this file was written, whichever location is chosen now: changing the choice moves
    -- nothing, so every file stays where it landed.
    storage_location_id UUID NOT NULL
        CONSTRAINT page_attachments_storage_location REFERENCES storage_locations (id),
    -- Relative to the location's directory.
    storage_path TEXT NOT NULL,
    -- The name people see, cleaned of anything a path or a header could read as more than a name.
    file_name TEXT NOT NULL CHECK (char_length(file_name) BETWEEN 1 AND 200),
    -- The type it is served as, set from the file's extension by the API's one table, never from
    -- what the browser that sent it said.
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    created_by UUID NOT NULL REFERENCES users (id),
    -- The agent acting for them, as `page_versions` records it. NULL when they added it themselves.
    agent TEXT,
    agent_name TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    -- Set when it was taken off its page. Its file stays with the provider, so it can be put back.
    deleted_at TIMESTAMPTZ,
    CONSTRAINT page_attachments_agent_named_with_its_agent CHECK (
        agent IS NOT NULL OR agent_name IS NULL
    )
);

-- Every read of a page lists what it holds, in the order it was added.
CREATE INDEX page_attachments_of_page ON page_attachments (page_id, created_at)
    WHERE deleted_at IS NULL;

-- Deleting a location refuses while files are in it, and the search for them starts from it.
CREATE INDEX page_attachments_location ON page_attachments (storage_location_id);

ALTER TABLE workspace_settings
    ADD COLUMN page_attachment_storage_location_id UUID
        CONSTRAINT workspace_settings_page_attachment_storage_location
            REFERENCES storage_locations (id);
