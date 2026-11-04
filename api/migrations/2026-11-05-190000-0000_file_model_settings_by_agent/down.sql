ALTER TABLE model_settings
    RENAME CONSTRAINT model_settings_created_by_fkey TO step_model_settings_created_by_fkey;
ALTER TABLE model_settings RENAME CONSTRAINT model_settings_effort_check TO step_model_settings_effort_check;
ALTER TABLE model_settings RENAME CONSTRAINT model_settings_model_check TO step_model_settings_model_check;
ALTER TABLE model_settings RENAME CONSTRAINT model_settings_agent_check TO step_model_settings_step_check;
ALTER INDEX model_settings_pkey RENAME TO step_model_settings_pkey;

ALTER TABLE model_settings RENAME COLUMN agent TO step;
ALTER TABLE model_settings RENAME TO step_model_settings;
