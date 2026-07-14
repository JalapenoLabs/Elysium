-- Org branding for white-labeling, alongside the existing name and logo.
--
-- `accent_color` is a `#RRGGBB` hex (validated in the API) that drives per-org
-- theming; `wordmark` is the short brand text shown in place of "Elysium". Both
-- default to empty (fall back to the Elysium defaults) and ride the same
-- `org.updated` realtime fan-out as name and logo (docs/realtime.md).

ALTER TABLE orgs
    ADD COLUMN accent_color TEXT NOT NULL DEFAULT '',
    ADD COLUMN wordmark     TEXT NOT NULL DEFAULT '';
