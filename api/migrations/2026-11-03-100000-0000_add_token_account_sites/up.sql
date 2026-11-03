-- A provider whose instance each account names, such as a YouTrack Cloud site, keeps it here as
-- an https origin. The API validates the host before it is stored (`crate::youtrack`); this only
-- keeps the column an origin and nothing more.
ALTER TABLE token_accounts
    ADD COLUMN site_url TEXT CHECK (site_url ~ '^https://[a-z0-9][a-z0-9.-]{0,240}$');

-- A YouTrack key means nothing without the site it was issued on.
ALTER TABLE token_accounts
    ADD CONSTRAINT token_accounts_youtrack_site CHECK (kind <> 'youtrack' OR site_url IS NOT NULL);
