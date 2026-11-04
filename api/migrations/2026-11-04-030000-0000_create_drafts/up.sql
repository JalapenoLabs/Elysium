-- Where a reply goes in a chat: in the thread of the message it answers, or as a new message in
-- that message's conversation.
CREATE TYPE reply_place AS ENUM ('thread', 'conversation');

-- How far a draft got. A draft is written, then sent; one that failed keeps its text and the
-- reason, and can be sent again. `sending` is held only while one send is under way, so a second
-- press of Send cannot post the message twice.
CREATE TYPE draft_state AS ENUM ('draft', 'sending', 'sent', 'failed');

-- A message a person sends as themselves: a reply through the connector a record came from, such
-- as Slack or Teams, or mail through one of their mailboxes. It is a draft until the person sends
-- it, and is kept afterwards with the record its sending put on the timeline. See
-- docs/connectors.md, Sending.
CREATE TABLE drafts (
    id UUID PRIMARY KEY,
    -- The record it answers, when it is a reply. A record removed later leaves the draft, which
    -- still knows where it goes.
    reply_to_record_id UUID REFERENCES timeline_records (id) ON DELETE SET NULL,
    -- What sends it: a connector, or a mailbox. Exactly one is set; removing either removes the
    -- drafts that would go through it, which could no longer be sent.
    connector_id UUID REFERENCES connectors (id) ON DELETE CASCADE,
    mail_account_id UUID REFERENCES mail_accounts (id) ON DELETE CASCADE,
    -- Where it goes, in its sender's own terms: a Slack channel and thread, a Teams conversation,
    -- or a mail's recipients and subject. Decided when the draft is written.
    target JSONB NOT NULL,
    -- For a chat reply, where in the conversation it goes.
    place reply_place,
    body TEXT NOT NULL,
    state draft_state NOT NULL DEFAULT 'draft',
    -- Why the last send failed, as the provider or Elysium said; set exactly while `failed`.
    last_error TEXT,
    -- When a send last started.
    attempted_at TIMESTAMPTZ,
    sent_at TIMESTAMPTZ,
    -- The record the sent message became.
    sent_record_id UUID REFERENCES timeline_records (id) ON DELETE SET NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT drafts_one_sender CHECK (num_nonnulls(connector_id, mail_account_id) = 1),
    CONSTRAINT drafts_body_length CHECK (char_length(body) BETWEEN 1 AND 20000),
    CONSTRAINT drafts_target_object CHECK (jsonb_typeof(target) = 'object'),
    CONSTRAINT drafts_error_while_failed CHECK ((state = 'failed') = (last_error IS NOT NULL)),
    CONSTRAINT drafts_error_length CHECK (char_length(last_error) <= 2000),
    CONSTRAINT drafts_sent_at_while_sent CHECK ((state = 'sent') = (sent_at IS NOT NULL))
);

SELECT diesel_manage_updated_at('drafts');

-- A record's drafts, which its reply box shows.
CREATE INDEX drafts_by_record ON drafts (reply_to_record_id, created_at DESC);
-- A person's drafts.
CREATE INDEX drafts_by_person ON drafts (created_by, created_at DESC);
