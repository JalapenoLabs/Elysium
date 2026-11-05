-- Takes back the version up.sql added, and nothing an admin wrote. Once an admin has written over
-- it, it stays as the history they worked from: their version may even have been restored from it.
DELETE FROM agent_guidance_versions
WHERE version = 2
  AND created_by = '00000000-0000-0000-0000-000000000001'
  AND NOT EXISTS (SELECT 1 FROM agent_guidance_versions WHERE version > 2);
