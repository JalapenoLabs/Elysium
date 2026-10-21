-- A one or two sentence summary of each finished turn, written by a small model after the turn
-- and shown as the turn's collapsed row. See docs/coding.md, Turn summaries.
--
-- A child of coding_sessions, so it inherits its session's creator rather than recording one.
CREATE TABLE turn_summaries (
    session_id BIGINT NOT NULL REFERENCES coding_sessions (id) ON DELETE CASCADE,
    turn_id UUID NOT NULL,
    summary TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (session_id, turn_id),
    CONSTRAINT turn_summaries_summary_length CHECK (char_length(summary) BETWEEN 1 AND 500)
);
