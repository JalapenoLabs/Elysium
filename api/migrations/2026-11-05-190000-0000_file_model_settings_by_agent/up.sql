-- Model settings cover every agent, not only workflow steps: a step whose work is an agent turn,
-- and the agents that run outside the step framework (Elysia, coding and Studio sessions, turn
-- summaries, meeting segmentation). One table keyed by the agent's name in code holds them all,
-- so overrides per project (on the roadmap) join on that one name. A step's name is its agent's
-- name, so every row stored so far keeps its meaning. See docs/engine.md, Models.
ALTER TABLE step_model_settings RENAME TO model_settings;
ALTER TABLE model_settings RENAME COLUMN step TO agent;

-- The constraints keep the names the table had; they say what they check under the new ones.
ALTER INDEX step_model_settings_pkey RENAME TO model_settings_pkey;
ALTER TABLE model_settings RENAME CONSTRAINT step_model_settings_step_check TO model_settings_agent_check;
ALTER TABLE model_settings RENAME CONSTRAINT step_model_settings_model_check TO model_settings_model_check;
ALTER TABLE model_settings RENAME CONSTRAINT step_model_settings_effort_check TO model_settings_effort_check;
ALTER TABLE model_settings
    RENAME CONSTRAINT step_model_settings_created_by_fkey TO model_settings_created_by_fkey;
