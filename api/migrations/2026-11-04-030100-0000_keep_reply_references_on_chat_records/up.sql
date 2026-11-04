-- A Slack or Teams record now keeps, in its metadata, where its message was said, so a reply can
-- be sent to it. The readers write it on every new record; this fills it in on the records they
-- wrote before, from what those already hold. See docs/connectors.md, Sending.

-- Slack: `external_id` is `<team>/<conversation>/<ts>`, and a reply's permalink carries the
-- thread it is in as `thread_ts`. A message that is not in a thread keeps no `threadTs`.
UPDATE timeline_records
SET metadata = metadata || jsonb_strip_nulls(jsonb_build_object(
    'team', split_part(external_id, '/', 1),
    'conversation', split_part(external_id, '/', 2),
    'ts', split_part(external_id, '/', 3),
    'threadTs', substring(url FROM 'thread_ts=([0-9]+\.[0-9]+)')
))
WHERE source_kind = 'slack'
    AND external_id ~ '^[^/]+/[^/]+/[0-9]+\.[0-9]+$'
    AND NOT metadata ? 'ts';

-- Teams: `external_id` is `<conversation>/<message>`. Which thread a channel message was in was
-- not kept, so these answer as the thread's first message; whether the conversation is a chat
-- follows the reader's own rule for a conversation without a thread type (`teams::is_chat`).
UPDATE timeline_records
SET metadata = metadata || jsonb_build_object(
    'conversationId', regexp_replace(external_id, '/[^/]*$', ''),
    'messageId', substring(external_id FROM '[^/]*$'),
    'isChat', (
        regexp_replace(external_id, '/[^/]*$', '') LIKE '%@unq.gbl.spaces'
        OR regexp_replace(external_id, '/[^/]*$', '') LIKE '%@thread.v2'
    )
)
WHERE source_kind = 'teams'
    AND external_id ~ '^.+/[^/]+$'
    AND NOT metadata ? 'messageId';
