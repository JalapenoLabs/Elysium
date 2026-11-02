-- Claude accounts signed in through OAuth, beside API keys and setup-tokens.
--
-- A `claude_account` credential is an ordinary `llms` row, so it takes part in the failover
-- stack by priority like every other credential, and its `secret_token_encrypted` holds the
-- current access token, the one bearer a turn is handed. Everything Elysium needs to keep that
-- token fresh lives here instead: the refresh token, both expiries, whose account it is, and
-- the latest usage Anthropic reported for it. Elysium is the only process that ever refreshes:
-- Claude Code rotates the refresh token on every refresh, so a second refresher would strand
-- the other.
ALTER TYPE llm_type ADD VALUE 'claude_account';

CREATE TABLE claude_accounts (
    -- One per credential; deleting the credential deletes its account.
    llm_id UUID PRIMARY KEY REFERENCES llms (id) ON DELETE CASCADE,
    -- Anthropic's account id. Signing the same account in again replaces its tokens.
    account_uuid UUID NOT NULL,
    email TEXT NOT NULL,
    organization_uuid UUID,
    organization_name TEXT,
    -- As Claude Code reports it, such as `max` or `pro`.
    subscription_type TEXT,
    -- Sealed by the API under this row's credential id; Postgres only ever holds ciphertext.
    refresh_token_encrypted BYTEA NOT NULL,
    access_token_expires_at TIMESTAMPTZ NOT NULL,
    -- NULL when Anthropic did not say.
    refresh_token_expires_at TIMESTAMPTZ,
    -- `expired` once Anthropic refused the refresh token. Only a new sign-in clears it.
    sign_in_state TEXT NOT NULL DEFAULT 'signed_in',
    last_refreshed_at TIMESTAMPTZ,
    -- Why the last refresh failed; NULL after one that succeeded.
    last_refresh_error TEXT,
    -- The latest usage, as percentages of each window, and when each window resets.
    five_hour_utilization DOUBLE PRECISION,
    five_hour_resets_at TIMESTAMPTZ,
    seven_day_utilization DOUBLE PRECISION,
    seven_day_resets_at TIMESTAMPTZ,
    usage_read_at TIMESTAMPTZ,
    -- Why the last usage read failed; NULL after one that succeeded.
    usage_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT claude_accounts_account_unique UNIQUE (account_uuid),
    CONSTRAINT claude_accounts_email_length CHECK (char_length(email) BETWEEN 1 AND 320),
    CONSTRAINT claude_accounts_sign_in_state CHECK (sign_in_state IN ('signed_in', 'expired')),
    -- Version byte + 24-byte nonce + 16-byte tag is the smallest possible sealed value.
    CONSTRAINT claude_accounts_refresh_token_sealed CHECK (octet_length(refresh_token_encrypted) >= 41),
    CONSTRAINT claude_accounts_utilization_range CHECK (
        (five_hour_utilization IS NULL OR five_hour_utilization >= 0)
        AND (seven_day_utilization IS NULL OR seven_day_utilization >= 0)
    )
);

SELECT diesel_manage_updated_at('claude_accounts');
