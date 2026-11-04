-- Learning categories: Elysium's nine built-in ones, always present, and the categories each
-- workspace adds of its own, such as its subject areas. Every category is a row, so a learning
-- references the one it is filed under and Postgres itself keeps any learning from losing it.
-- See docs/knowledge.md, Categories.

CREATE TABLE learning_categories (
    -- What a learning stores and an agent sends: lowercase words joined by underscores. It is
    -- written into learnings and their history, so it never changes once created.
    key TEXT PRIMARY KEY CHECK (key ~ '^[a-z][a-z0-9_]{0,49}$'),
    -- What people read. The web app translates a built-in category's by its key.
    label TEXT NOT NULL CHECK (char_length(label) BETWEEN 1 AND 60),
    -- What belongs there, written for the agent choosing a category for what it learned.
    description TEXT NOT NULL CHECK (char_length(description) BETWEEN 1 AND 500),
    -- The order categories are offered in: the built-in ones first, in theirs, then the
    -- workspace's own in the order its admins gave them.
    position INTEGER NOT NULL CHECK (position >= 0),
    -- One of the nine Elysium ships. They are seeded here and never changed or removed.
    built_in BOOLEAN NOT NULL DEFAULT false,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('learning_categories');

-- The nine built-in categories, created by the system user, with the descriptions agents have
-- read since learnings were built.
INSERT INTO learning_categories (key, label, description, position, built_in, created_by) VALUES
    ('decision', 'Decision',
        'A choice that was made and why, so it is not argued again.',
        0, true, '00000000-0000-0000-0000-000000000001'),
    ('convention', 'Convention',
        'How work is done here: style, naming, process, review.',
        1, true, '00000000-0000-0000-0000-000000000001'),
    ('procedure', 'Procedure',
        'Steps known to work for a task that comes back: setting up, deploying, restoring.',
        2, true, '00000000-0000-0000-0000-000000000001'),
    ('pitfall', 'Pitfall',
        'Something that behaves other than expected, and the way around it.',
        3, true, '00000000-0000-0000-0000-000000000001'),
    ('reference', 'Reference',
        'Facts about a tool, API, or service worth not looking up again: limits, endpoints, flags.',
        4, true, '00000000-0000-0000-0000-000000000001'),
    ('environment', 'Environment',
        'Where things are: hosts, paths, accounts, and how they connect.',
        5, true, '00000000-0000-0000-0000-000000000001'),
    ('people', 'People',
        'Who owns, reviews, or decides what, and how they want work delivered.',
        6, true, '00000000-0000-0000-0000-000000000001'),
    ('project', 'Project',
        'A project''s or a client''s goals, constraints, and current state.',
        7, true, '00000000-0000-0000-0000-000000000001'),
    ('incident', 'Incident',
        'What broke, why, and what fixed it.',
        8, true, '00000000-0000-0000-0000-000000000001');

-- Every existing learning keeps its category: the enum's values are the built-in keys.
-- NO ACTION rather than RESTRICT, so a category still in use is refused with the foreign key
-- violation Diesel recognizes; removing one moves its learnings first (docs/knowledge.md).
ALTER TABLE learnings
    ALTER COLUMN category TYPE TEXT USING category::text,
    ADD CONSTRAINT learnings_category_fkey
        FOREIGN KEY (category) REFERENCES learning_categories (key);

-- Counting a category's learnings, and moving them when it is removed.
CREATE INDEX learnings_category_idx ON learnings (category);

DROP TYPE learning_category;
