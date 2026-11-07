DELETE FROM work_batch_events WHERE kind = 'requeued';
ALTER TABLE work_batch_events DROP CONSTRAINT work_batch_events_kind_known;
ALTER TABLE work_batch_events ADD CONSTRAINT work_batch_events_kind_known CHECK (kind IN (
    'submitted', 'held_for_approval', 'edited', 'approved', 'rejected', 'pinned', 'unpinned',
    'reprioritized', 'paused', 'resumed', 'cancelled', 'assigned', 'unassigned', 'started',
    'waiting', 'answered', 'finished'
));

ALTER TABLE work_batches
    DROP CONSTRAINT work_batches_configuration_object,
    DROP COLUMN configuration;

DROP TABLE agent_configuration_status;

ALTER TABLE workspace_settings DROP COLUMN agent_configuration_github_credential_id;
