-- `updated_at` says when what a page holds last changed, which is how the Learnings list sorts.
-- A move or a renumbering changes where a page sits and nothing it holds, so it leaves the column
-- alone. The trigger reads the row's place rather than listing its content, so a column added
-- later counts as content without another migration.
DROP TRIGGER set_updated_at ON pages;
CREATE TRIGGER set_updated_at BEFORE UPDATE ON pages
    FOR EACH ROW
    WHEN (OLD.parent_id IS NOT DISTINCT FROM NEW.parent_id AND OLD.position = NEW.position)
    EXECUTE PROCEDURE diesel_set_updated_at();
