-- The global search finds action items, projects, coding sessions, and initiatives by their
-- titles (`GET /api/v1/search`; docs/api.md, Search). Each typed word must begin a word of the
-- title, matched as `title ~* '\mword'`, and pg_trgm's GIN indexes answer that regular
-- expression. Full text was the other candidate, but its parser keeps `WAY-123` as `way` and
-- `-123`, and a host or a path such as `src/main.rs` as one token, so the ticket keys, hosts,
-- and paths these titles hold would not be found by their parts. pg_trgm is a trusted extension.
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- Each index covers exactly the rows its list route shows, which the search's query repeats, so
-- the planner can use it: items and initiatives not deleted, and sessions outside Studio.
CREATE INDEX action_items_title_search
    ON action_items USING gin (title gin_trgm_ops)
    WHERE deleted_at IS NULL;
CREATE INDEX projects_name_search
    ON projects USING gin (name gin_trgm_ops);
CREATE INDEX coding_sessions_title_search
    ON coding_sessions USING gin (title gin_trgm_ops)
    WHERE studio_item_id IS NULL;
CREATE INDEX initiatives_name_search
    ON initiatives USING gin (name gin_trgm_ops)
    WHERE deleted_at IS NULL;
