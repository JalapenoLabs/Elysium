-- The password vault agents sign in to things with: one Bitwarden or Vaultwarden account the API
-- reaches through the official `bw` CLI, of which agents may read one collection alone. See
-- docs/passwords.md.

-- Where the account lives. Bitwarden's own servers are fixed in code; a self-hosted server
-- (Vaultwarden, or Bitwarden's own self-hosted edition) names its origin.
CREATE TYPE password_server AS ENUM ('bitwarden_us', 'bitwarden_eu', 'self_hosted');

-- One row: Elysium holds one vault account.
CREATE TABLE password_vault (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE,
    server password_server NOT NULL,
    -- A self-hosted server's origin, https; NULL for Bitwarden's own servers.
    server_url TEXT,
    -- The account's personal API key: its client_id, `user.<uuid>`, which is no secret on its
    -- own, and its client_secret, sealed with the encryption key.
    client_id TEXT NOT NULL,
    client_secret_encrypted BYTEA NOT NULL,
    -- The master password, sealed with the encryption key. The CLI needs it to decrypt the vault
    -- after every start; it is never written anywhere else.
    master_password_encrypted BYTEA NOT NULL,
    -- The account the key signs in as, as the CLI reported it when the key was last checked.
    account_email TEXT NOT NULL,
    -- The one collection agents may read, and the organization it belongs to. NULL until an admin
    -- chooses it, and until then agents can read nothing.
    collection_id UUID,
    collection_name TEXT,
    organization_id UUID,
    -- When the CLI last confirmed the key and the master password.
    checked_at TIMESTAMPTZ NOT NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT password_vault_one_row CHECK (singleton),
    CONSTRAINT password_vault_server_url CHECK (
        (server = 'self_hosted') = (server_url IS NOT NULL)
        AND (server_url IS NULL OR (server_url LIKE 'https://%' AND char_length(server_url) <= 2048))
    ),
    CONSTRAINT password_vault_client_id CHECK (char_length(client_id) BETWEEN 1 AND 200),
    CONSTRAINT password_vault_account_email CHECK (char_length(account_email) BETWEEN 1 AND 320),
    CONSTRAINT password_vault_collection_whole CHECK (
        (collection_id IS NULL) = (collection_name IS NULL)
        AND (collection_id IS NULL) = (organization_id IS NULL)
    ),
    CONSTRAINT password_vault_collection_name CHECK (char_length(collection_name) <= 1000)
);

SELECT diesel_manage_updated_at('password_vault');
