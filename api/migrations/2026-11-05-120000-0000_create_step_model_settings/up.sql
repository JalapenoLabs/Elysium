-- The model and effort each workflow step's agent turn runs on. A step without a row runs on the
-- default in code (Opus 5.5 at high effort), so a row exists only for a step an admin changed.
-- See docs/engine.md, Models.
CREATE TABLE step_model_settings (
    -- The step's name in code, such as `correct_transcript`, as `step_jobs.step` names it.
    step TEXT PRIMARY KEY CHECK (step ~ '^[a-z][a-z0-9_]{0,63}$'),
    -- What the official CLI's `--model` takes: an id such as `claude-opus-5-5`, or an alias. It
    -- starts with a letter or digit, so it can never read as another flag.
    model TEXT NOT NULL CHECK (model ~ '^[A-Za-z0-9][A-Za-z0-9._\[\]-]{0,99}$'),
    -- What the CLI's `--effort` takes. Text rather than an enum, so a level Anthropic adds needs
    -- no enum migration.
    effort TEXT NOT NULL CHECK (effort IN ('low', 'medium', 'high', 'xhigh', 'max')),
    -- The admin who first set it.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('step_model_settings');
