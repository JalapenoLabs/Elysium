-- Two people whose connectors read the same channel or space each have the record, so the index
-- holds the same text twice, as two documents of one source with the same content_sha256. A search
-- folds them into one result, and a document's page lists its copies, found through this index
-- however large the index grows. See docs/knowledge.md, The same text twice.
CREATE INDEX knowledge_documents_copies ON knowledge_documents (content_sha256);
