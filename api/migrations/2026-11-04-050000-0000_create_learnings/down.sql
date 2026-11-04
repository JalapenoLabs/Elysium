-- A learning's documents go with the column; the timeline records' documents stay.
DELETE FROM knowledge_documents WHERE learning_id IS NOT NULL;

ALTER TABLE knowledge_documents DROP CONSTRAINT knowledge_documents_one_source;
ALTER TABLE knowledge_documents DROP COLUMN learning_id;
ALTER TABLE knowledge_documents ADD CONSTRAINT knowledge_documents_one_source
    CHECK (num_nonnulls(timeline_record_id) = 1);

DROP TABLE learning_events;
DROP TABLE learnings;
DROP TYPE learning_category;
