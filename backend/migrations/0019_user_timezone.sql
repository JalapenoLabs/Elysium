-- The user's preferred display timezone, as an IANA name (e.g. "America/Denver").
--
-- Empty means auto-detect: the client falls back to the browser's guessed zone.
-- Stored as the IANA identifier rather than an offset or an enum because the tz
-- database ships several updates a year and offsets drift with DST; see
-- docs/proto.md's timestamp standard. The value is display-only and never alters
-- a stored instant, which is always UTC.

ALTER TABLE users ADD COLUMN timezone TEXT NOT NULL DEFAULT '';
