-- A project at Full autonomy wrote to its trackers directly, so it writes directly again. Approve
-- found work only has no counterpart before the dial: the batches it submits wait for a person
-- again, as every project's did then, and its changes were proposed already.
ALTER TABLE projects
    ADD COLUMN manager_writes_directly BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE projects SET manager_writes_directly = TRUE WHERE autonomy = 'full_autonomy';

DROP TABLE project_autonomy_changes;
ALTER TABLE projects DROP COLUMN autonomy;
DROP TYPE project_autonomy;
