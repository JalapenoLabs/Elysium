-- Every Fireflies key belongs to a Fireflies connector, so a meeting source is added, checked,
-- paused, and removed from Connectors alone, like every other connector. See docs/meetings.md.
--
-- Removing a connector once left its account behind, so a removed Fireflies connector's key can
-- still be stored with nothing in the web app that reaches it. Each such key becomes a Fireflies
-- connector that owns it, paused: its person removed the connector, so nothing is read until they
-- resume it. The key is kept, so resuming needs no re-entry. The connector names no storage
-- location until its person picks one on its page; until then its recordings are not kept.
CREATE TEMPORARY TABLE adopted_keys AS
SELECT uuidv7() AS connector_id,
       token_accounts.id AS token_account_id,
       token_accounts.created_by,
       token_accounts.created_at
FROM token_accounts
WHERE token_accounts.kind = 'fireflies'
  AND NOT EXISTS (
      SELECT 1 FROM connectors WHERE connectors.token_account_id = token_accounts.id
  );

INSERT INTO connectors (id, kind, token_account_id, is_active, created_by, created_at)
SELECT connector_id, 'fireflies', token_account_id, FALSE, created_by, created_at
FROM adopted_keys;

-- A removed connector's meetings were kept without it. A person with one adopted key gets theirs
-- back on its connector, so resuming it never reads and processes them a second time. A person
-- with several cannot be told which key read which, so theirs stay as they are. Of a meeting kept
-- more than once, the newest copy is attached, since a connector holds each meeting once.
WITH sole_keys AS (
    SELECT adopted.created_by, adopted.connector_id
    FROM adopted_keys AS adopted
    WHERE NOT EXISTS (
        SELECT 1 FROM adopted_keys AS other
        WHERE other.created_by = adopted.created_by
          AND other.connector_id <> adopted.connector_id
    )
),
unowned AS (
    SELECT DISTINCT ON (meetings.created_by, meetings.external_id)
           meetings.id,
           sole_keys.connector_id
    FROM meetings
    JOIN sole_keys ON sole_keys.created_by = meetings.created_by
    WHERE meetings.connector_id IS NULL
      AND meetings.source_kind = 'fireflies'
    ORDER BY meetings.created_by, meetings.external_id, meetings.created_at DESC, meetings.id
)
UPDATE meetings
SET connector_id = unowned.connector_id
FROM unowned
WHERE meetings.id = unowned.id;

DROP TABLE adopted_keys;
