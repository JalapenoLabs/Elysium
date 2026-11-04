DROP TABLE work_batch_pull_requests;

DROP TYPE pull_request_merge_method;

DROP TYPE pull_request_state;

ALTER TABLE project_repositories DROP COLUMN merge_policy;

DROP TYPE merge_policy;
