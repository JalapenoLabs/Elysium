-- Agents write pages now, so the default guidance says when one is better than a learning. How to
-- use the tools is in their descriptions; when to reach for one is the guidance's to say.
--
-- A release never replaces what an admin wrote (docs/knowledge.md, Agent guidance), so this adds
-- the next version only while the seed is still the only one. A deployment whose admins saved or
-- restored anything keeps exactly what they chose, and they add the paragraph themselves if they
-- want it. Written by the system user, as the seed was.
INSERT INTO agent_guidance_versions (version, content, created_by)
SELECT 2,
       content || E'\n\n' ||
       'Write a page with page_save, rather than a learning, for what people will read as a whole '
       'and keep up to date, such as a runbook, a design, or how a system works; a learning is one '
       'takeaway, found when it is needed. Find the page that already covers it and change that '
       'page rather than starting another.',
       '00000000-0000-0000-0000-000000000001'
FROM agent_guidance_versions
WHERE version = 1
  AND NOT EXISTS (SELECT 1 FROM agent_guidance_versions WHERE version > 1);
