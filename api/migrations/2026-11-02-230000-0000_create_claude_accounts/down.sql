DROP TABLE claude_accounts;

-- Postgres cannot drop one value from an enum, so the type is rebuilt without it. The
-- credentials of that type go first: nothing else can hold them.
DELETE FROM llms WHERE type = 'claude_account';

ALTER TYPE llm_type RENAME TO llm_type_with_claude_account;
CREATE TYPE llm_type AS ENUM (
    'chatgpt_oauth',
    'chatgpt_api_token',
    'claude_api_token',
    'claude_code_oauth'
);
ALTER TABLE llms ALTER COLUMN type TYPE llm_type USING type::text::llm_type;
DROP TYPE llm_type_with_claude_account;
