-- An entry naming a label is about a table that no longer exists, and no version before this one
-- reads those kinds.
DELETE FROM page_events WHERE kind IN ('labeled', 'unlabeled');

DROP TABLE page_labels;
