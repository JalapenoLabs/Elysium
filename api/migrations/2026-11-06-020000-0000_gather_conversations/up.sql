-- Conversations: where direct messages and mentions are gathered per conversation before work
-- starts on them, and where the agent handling a conversation keeps its session. Every message
-- that asks something of a person joins its conversation as it is ingested; the engine starts a
-- run of the request workflow once the conversation has been quiet for ten seconds, and messages
-- that arrive while that run is live are delivered to its handling agent. Rows, not timers, so a
-- restarted engine picks up exactly where its predecessor stopped. See docs/engine.md,
-- Conversations.
CREATE TABLE conversations (
    -- UUIDv7.
    id UUID PRIMARY KEY,
    -- The connector its messages are read through. Removing the connector removes its
    -- conversations, as it removes the records they gather.
    connector_id UUID NOT NULL REFERENCES connectors (id) ON DELETE CASCADE,
    -- The conversation in its kind's own terms: a Slack direct message or channel thread, a
    -- Teams chat or channel thread, a mail thread by sender and subject.
    key TEXT NOT NULL CHECK (char_length(key) BETWEEN 1 AND 1024),
    -- What it is called where it is had, as its newest message named it, such as
    -- `#launch in Mooreslab`.
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 2048),
    -- The Claude session its handling agent's turns resume; null until its first turn.
    harness_session_id TEXT CHECK (char_length(harness_session_id) BETWEEN 1 AND 128),
    -- When a wait its handling agent asked for ends, after which the agent is owed a turn; null
    -- while nothing is waited out.
    waiting_until TIMESTAMPTZ,
    -- The run whose agent asked to wait, so it waits at most once per run. A run is never
    -- removed; were one, the conversation could wait once more.
    waited_in_run_id UUID REFERENCES workflow_runs (id) ON DELETE SET NULL,
    -- Whose conversation it is: the connector's person, whose timeline its messages are on.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE (connector_id, key)
);

CREATE INDEX conversations_created_by ON conversations (created_by);

-- Each message a conversation gathered: a timeline record, and how far work on it got. A child of
-- its conversation, so it takes its owner from there.
CREATE TABLE conversation_messages (
    -- A record belongs to at most one conversation.
    timeline_record_id UUID PRIMARY KEY REFERENCES timeline_records (id) ON DELETE CASCADE,
    conversation_id UUID NOT NULL REFERENCES conversations (id) ON DELETE CASCADE,
    -- Whether it asks something of the person (a direct message, a mention, a reply in a thread
    -- they follow), which is what starts work. One that does not, such as the person's own reply,
    -- only joins a conversation already being worked on, as context.
    wakes BOOLEAN NOT NULL,
    -- When Elysium read it: a conversation is quiet once ten seconds passed since its newest.
    received_at TIMESTAMPTZ NOT NULL,
    -- The request run that took it, whose segmenting reads it; null until one does.
    run_id UUID REFERENCES workflow_runs (id) ON DELETE SET NULL,
    -- When a handling agent was given it; null until one is. A message that arrives while a run
    -- is live is given to that run's agent before any run takes it.
    delivered_at TIMESTAMPTZ
);

-- What still waits on a run or an agent, per conversation, in the order it arrived: the engine's
-- look for quiet conversations and the agent's look for what it has not been given.
CREATE INDEX conversation_messages_waiting ON conversation_messages (conversation_id, received_at)
    WHERE run_id IS NULL OR delivered_at IS NULL;
-- A conversation's messages in order, for the context a run reads besides what it takes.
CREATE INDEX conversation_messages_conversation ON conversation_messages (conversation_id, received_at);
