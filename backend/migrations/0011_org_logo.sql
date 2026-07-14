-- An org's broadcastable branding: a logo URL, alongside its name.
--
-- Editing either fans out an `org.updated` realtime event to connected members
-- (see docs/realtime.md). The URL is a plain string (local storage path or an
-- external URL); object-storage integration is a separate concern.

ALTER TABLE orgs ADD COLUMN logo_url TEXT NOT NULL DEFAULT '';
