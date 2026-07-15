-- Contact and personalization fields on the user profile, all optional.
--
-- `language` stores the `elysium.user.v1.Language` enum value (0 = auto-detect,
-- the default); the proto enum is the source of truth for the codes. The rest are
-- free text, empty when unset, mirroring the existing `bio` / `display_name` shape.
-- `backup_email` and `contact_phone` are stored now; verification rides the future
-- email / SMS integrations.

ALTER TABLE users
    ADD COLUMN pronouns      TEXT     NOT NULL DEFAULT '',
    ADD COLUMN web_link      TEXT     NOT NULL DEFAULT '',
    ADD COLUMN contact_phone TEXT     NOT NULL DEFAULT '',
    ADD COLUMN backup_email  TEXT     NOT NULL DEFAULT '',
    ADD COLUMN language      SMALLINT NOT NULL DEFAULT 0;
