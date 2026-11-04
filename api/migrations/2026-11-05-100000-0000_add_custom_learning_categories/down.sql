-- Back to the fixed list. A learning filed under one of the workspace's own categories has no
-- place in it, so rather than refile or drop anyone's learning, the rollback refuses until every
-- such learning has been moved to a built-in category.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM learnings
        JOIN learning_categories ON learning_categories.key = learnings.category
        WHERE NOT learning_categories.built_in
    ) THEN
        RAISE EXCEPTION 'learnings are filed under custom categories; move them to built-in '
            'categories before rolling back';
    END IF;
END
$$;

CREATE TYPE learning_category AS ENUM (
    'decision',
    'convention',
    'procedure',
    'pitfall',
    'reference',
    'environment',
    'people',
    'project',
    'incident'
);

DROP INDEX learnings_category_idx;

ALTER TABLE learnings
    DROP CONSTRAINT learnings_category_fkey,
    ALTER COLUMN category TYPE learning_category USING category::learning_category;

DROP TABLE learning_categories;
