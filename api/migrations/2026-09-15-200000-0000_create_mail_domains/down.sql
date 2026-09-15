DELETE FROM mail_accounts WHERE kind = 'self_hosted';
DROP INDEX mail_accounts_mail_domain_id;
ALTER TABLE mail_accounts
    DROP CONSTRAINT mail_accounts_mail_domain_self_hosted,
    DROP COLUMN mail_domain_id;
DROP TABLE mail_domains;
DROP TABLE mail_servers;

CREATE TABLE mail_servers (
    id UUID PRIMARY KEY,
    domain TEXT NOT NULL,
    admin_username TEXT NOT NULL,
    admin_secret_encrypted BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT mail_servers_domain_shape CHECK (
        domain ~ '^[a-z0-9.-]+\.[a-z0-9-]+$' AND char_length(domain) <= 253
    ),
    CONSTRAINT mail_servers_admin_username_length CHECK (char_length(admin_username) BETWEEN 1 AND 320),
    CONSTRAINT mail_servers_admin_secret_sealed CHECK (octet_length(admin_secret_encrypted) >= 41)
);

CREATE UNIQUE INDEX mail_servers_singleton ON mail_servers ((true));
