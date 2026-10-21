-- The timeline: everything Elysium ingests, as one kind of record, attributed to projects. And
-- the connector accounts that ingest it: a service read through one of the person's browsers.
-- See docs/connectors.md.

CREATE TABLE connector_accounts (
    id UUID PRIMARY KEY,
    -- Which connector reads it, such as `outlook`.
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z][a-z0-9_]{0,31}$'),
    -- The browser it reads through. One connector per browser; removing the browser removes it.
    browser_session_id UUID NOT NULL UNIQUE REFERENCES browser_sessions (id) ON DELETE CASCADE,
    -- Whether the browser was signed in to the service when it was last read.
    sign_in_state TEXT NOT NULL DEFAULT 'unknown'
        CHECK (sign_in_state IN ('signed_in', 'signed_out', 'unknown')),
    -- Where the last read left off, in the connector's own terms.
    cursor JSONB NOT NULL DEFAULT '{}',
    last_polled_at TIMESTAMPTZ,
    -- Why the last read failed, or NULL when it succeeded.
    last_error TEXT,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('connector_accounts');

CREATE TABLE timeline_records (
    id UUID PRIMARY KEY,
    -- What kind of thing it is, such as `outlook`, `gmail`, `slack`, `meeting`, or `note`.
    source_kind TEXT NOT NULL CHECK (source_kind ~ '^[a-z][a-z0-9_]{0,31}$'),
    -- The account it was read from. Kept when the account is removed, so the history stays.
    connector_account_id UUID REFERENCES connector_accounts (id) ON DELETE SET NULL,
    -- The provider's own id for it, unique per account, so reading it twice records it once.
    external_id TEXT CHECK (char_length(external_id) <= 1024),
    occurred_at TIMESTAMPTZ NOT NULL,
    -- Who it came from, as the provider shows them, such as `Ada Lovelace <ada@example.com>`.
    actor TEXT NOT NULL DEFAULT '' CHECK (char_length(actor) <= 512),
    title TEXT NOT NULL DEFAULT '' CHECK (char_length(title) <= 2048),
    -- Plain text: a preview, a transcript segment, a note.
    body TEXT NOT NULL DEFAULT '',
    url TEXT CHECK (char_length(url) <= 4096),
    -- A span inside a recording, for a meeting segment.
    media JSONB,
    metadata JSONB NOT NULL DEFAULT '{}',
    -- The action item it raised, when it raised one.
    action_item_id UUID REFERENCES action_items (id) ON DELETE SET NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (connector_account_id, external_id)
);

CREATE INDEX timeline_records_occurred_at ON timeline_records (occurred_at DESC);

-- Which projects a record is about, with how sure the attribution is. Corrected by people; set
-- by Elysia.
CREATE TABLE timeline_record_projects (
    timeline_record_id UUID NOT NULL REFERENCES timeline_records (id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    -- Who attributed it, in the actor form history uses (`user:<id>`, `elysia`).
    attributed_by TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (timeline_record_id, project_id)
);

CREATE INDEX timeline_record_projects_project ON timeline_record_projects (project_id);
