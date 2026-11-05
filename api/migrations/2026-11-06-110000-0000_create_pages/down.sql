-- A page's documents go with the column; the other sources' documents stay.
DELETE FROM knowledge_documents WHERE page_id IS NOT NULL;

ALTER TABLE knowledge_documents DROP CONSTRAINT knowledge_documents_one_source;
ALTER TABLE knowledge_documents DROP COLUMN page_id;
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_one_source
    CHECK (num_nonnulls(timeline_record_id, learning_id, company_doc_section) = 1);

DROP TABLE page_events;
DROP TABLE page_versions;
DROP TABLE pages;
