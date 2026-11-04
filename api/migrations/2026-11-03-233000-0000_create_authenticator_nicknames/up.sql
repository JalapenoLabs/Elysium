-- The name a person gives their authenticator app, such as "Bitwarden", so they remember
-- which app holds Elysium's codes. Kratos holds the app itself, one per identity, and has
-- nowhere for a name, so the name lives beside the account it belongs to. It is removed when
-- the app is: by the person in Sign-in & security, or by an admin resetting it.
CREATE TABLE authenticator_nicknames (
    -- A child of the person's account; their own name for their own app.
    user_id UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    nickname TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT authenticator_nicknames_length
        CHECK (char_length(nickname) BETWEEN 1 AND 50 AND nickname = btrim(nickname))
);

SELECT diesel_manage_updated_at('authenticator_nicknames');
