-- Postgres cannot drop an enum value, so the provider type is rebuilt without it. The next
-- migration's rollback has already removed every YouTrack link and cursor. The checks naming a
-- provider compare its text, so they carry over to the rebuilt type unchanged.

ALTER TYPE link_provider RENAME TO link_provider_with_youtrack;
CREATE TYPE link_provider AS ENUM (
    'jira',
    'github'
);
ALTER TABLE action_item_links
    ALTER COLUMN provider TYPE link_provider USING provider::text::link_provider;
ALTER TABLE initiative_links
    ALTER COLUMN provider TYPE link_provider USING provider::text::link_provider;
DROP TYPE link_provider_with_youtrack;
