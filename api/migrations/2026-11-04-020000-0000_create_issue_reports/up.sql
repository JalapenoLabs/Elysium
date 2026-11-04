-- How a report reached Elysium's GitHub repository: as a new issue, or as a comment on the open
-- issue another report of the same problem opened first.
CREATE TYPE issue_report_filing AS ENUM ('issue', 'comment');

-- Every problem a person reported from the app to Elysium's own GitHub repository, with their
-- own GitHub token. Nothing that was posted is kept here: GitHub holds the report itself. A row
-- remembers which issue a problem's fingerprint went to, so a person is told when they already
-- reported it, the next report of the same problem finds the issue before GitHub's search has
-- indexed it, and each person's reports can be counted against the daily limit.
CREATE TABLE issue_reports (
    id UUID PRIMARY KEY,
    -- The problem, as the first 16 hex characters of a SHA-256 over its redacted, normalized
    -- error. Identical problems have identical fingerprints, whoever hit them.
    fingerprint TEXT NOT NULL,
    -- The issue the report went to, on the repository fixed in code.
    issue_number BIGINT NOT NULL,
    issue_url TEXT NOT NULL,
    filed_as issue_report_filing NOT NULL,
    -- Where this person's report reads: the issue they opened, or their comment on it.
    url TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT issue_reports_fingerprint_shape CHECK (fingerprint ~ '^[0-9a-f]{16}$'),
    CONSTRAINT issue_reports_issue_number_positive CHECK (issue_number > 0),
    CONSTRAINT issue_reports_urls_on_github CHECK (
        issue_url LIKE 'https://github.com/%' AND url LIKE 'https://github.com/%'
    )
);

-- A person's reports in the last day, for the limit.
CREATE INDEX issue_reports_by_person ON issue_reports (created_by, created_at DESC);
-- The issue a fingerprint last went to.
CREATE INDEX issue_reports_by_fingerprint ON issue_reports (fingerprint, created_at DESC);
