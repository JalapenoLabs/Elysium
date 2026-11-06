-- What pages' histories say of their links goes with the links: no entry may name a link the
-- schema can no longer hold.
DELETE FROM page_events WHERE kind IN ('project_linked', 'project_unlinked');

DROP TABLE page_projects;
