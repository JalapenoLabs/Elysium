-- Labels on knowledge base pages (`docs/knowledge.md`, Pages, Labels). What makes a label well
-- formed is the API's one rule (`crate::models::page_label::Label`); the database holds its length.
CREATE TABLE page_labels (
    page_id UUID NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    label TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (page_id, label),
    CONSTRAINT page_labels_label_length CHECK (char_length(label) BETWEEN 1 AND 50)
);

-- The search's label filter starts from a label and reads the pages carrying it.
CREATE INDEX page_labels_label ON page_labels (label, page_id);
