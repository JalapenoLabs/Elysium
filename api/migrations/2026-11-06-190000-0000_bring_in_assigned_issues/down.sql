UPDATE connectors SET content = '{}'::jsonb WHERE kind = 'github';

DROP TABLE assigned_issues;
