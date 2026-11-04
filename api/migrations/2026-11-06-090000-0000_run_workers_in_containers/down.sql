DELETE FROM work_batch_events WHERE kind IN ('waiting', 'answered');
ALTER TABLE work_batch_events DROP CONSTRAINT work_batch_events_kind_known;
ALTER TABLE work_batch_events ADD CONSTRAINT work_batch_events_kind_known CHECK (kind IN (
    'submitted', 'pinned', 'unpinned', 'reprioritized', 'paused', 'resumed', 'cancelled',
    'assigned', 'unassigned', 'started', 'finished'
));

ALTER TABLE work_batches
    DROP CONSTRAINT work_batches_worker_log_size,
    DROP CONSTRAINT work_batches_worker_report_object,
    DROP CONSTRAINT work_batches_waiting_whole,
    DROP CONSTRAINT work_batches_waiting_detail_length,
    DROP COLUMN worker_log,
    DROP COLUMN worker_report,
    DROP COLUMN waiting_until,
    DROP COLUMN waiting_detail,
    DROP COLUMN waiting_on,
    DROP COLUMN worker_session_id;

DROP TYPE work_batch_wait;

ALTER TABLE projects
    DROP CONSTRAINT projects_tools_setup_script_length,
    DROP COLUMN tools_setup_script;

DROP TABLE project_repositories;
