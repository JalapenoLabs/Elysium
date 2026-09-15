-- Which projects save files to a storage location: every project, including ones added
-- later, or only the projects linked below.

ALTER TABLE storage_locations ADD COLUMN all_projects BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE storage_location_projects (
    storage_location_id UUID NOT NULL REFERENCES storage_locations (id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (storage_location_id, project_id)
);

-- Finds a project's locations; the primary key already serves a location's projects.
CREATE INDEX storage_location_projects_project_id ON storage_location_projects (project_id);
