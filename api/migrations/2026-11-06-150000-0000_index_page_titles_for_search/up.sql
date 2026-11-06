-- The global search finds pages by their titles as they are typed, as it does every other record
-- (`GET /api/v1/search`; docs/database.md, Searching titles). The index covers exactly the pages
-- the tree shows, which the search's query repeats, so the planner can use it.
CREATE INDEX pages_title_search
    ON pages USING gin (title gin_trgm_ops)
    WHERE deleted_at IS NULL;
