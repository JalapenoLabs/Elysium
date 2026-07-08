-- Passkey (WebAuthn) credentials.
--
-- The API is the WebAuthn Relying Party (docs/auth-model.md), so it stores the
-- passkeys itself. Each row is a webauthn-rs `Passkey` serialized as JSON, bound
-- to a user. `credential_id` is the base64url credential id, indexed for lookup
-- and to reject duplicate registrations.

CREATE TABLE webauthn_credentials (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id       UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    passkey       JSONB NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at  TIMESTAMPTZ
);

CREATE INDEX webauthn_credentials_user_id_idx ON webauthn_credentials (user_id);
