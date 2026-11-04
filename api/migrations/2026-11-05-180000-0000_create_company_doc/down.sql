-- The doc's documents go with their column; the other sources' documents stay.
DELETE FROM knowledge_documents WHERE company_doc_section IS NOT NULL;

ALTER TABLE knowledge_documents DROP CONSTRAINT knowledge_documents_one_source;
ALTER TABLE knowledge_documents DROP COLUMN company_doc_section;
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_one_source
    CHECK (num_nonnulls(timeline_record_id, learning_id) = 1);

DROP INDEX timeline_records_by_ingestion;
DROP TABLE company_doc_review_cursor;
DROP TABLE company_doc_events;
DROP TABLE company_doc_proposals;
DROP TABLE company_doc_sections;
