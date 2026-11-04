-- YouTrack issues as action item links, read and written through a person's YouTrack token
-- account. A new enum value cannot be used in the transaction that adds it, so the columns and
-- checks that name it follow in the next migration.

ALTER TYPE link_provider ADD VALUE 'youtrack';
