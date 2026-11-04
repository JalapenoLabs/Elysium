-- Each repository a project declares says who merges its workers' pull requests: a person, the
-- default, or Elysium once every check passed and no review is left open. The pull requests a
-- worker opens are recorded against its batch, so Elysium can merge its own alone and put what
-- merged on the project's timeline. See docs/engine.md, Merging.

CREATE TYPE merge_policy AS ENUM ('person', 'auto');

ALTER TABLE project_repositories
    ADD COLUMN merge_policy merge_policy NOT NULL DEFAULT 'person';

CREATE TYPE pull_request_state AS ENUM ('open', 'merged', 'closed');

CREATE TYPE pull_request_merge_method AS ENUM ('squash', 'merge', 'rebase');

-- The pull requests a batch's worker opened, as it recorded them, and where each stands.
CREATE TABLE work_batch_pull_requests (
    id UUID PRIMARY KEY,
    work_batch_id UUID NOT NULL REFERENCES work_batches (id) ON DELETE CASCADE,
    -- `owner/name`, as GitHub spells it.
    repository TEXT NOT NULL,
    number INTEGER NOT NULL,
    -- Where a person reads it, on github.com.
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    state pull_request_state NOT NULL DEFAULT 'open',
    merged_at TIMESTAMPTZ,
    -- The login GitHub says merged it.
    merged_by TEXT,
    -- Whether Elysium merged it, and how, rather than a person on GitHub.
    merged_by_elysium BOOLEAN NOT NULL DEFAULT false,
    merge_method pull_request_merge_method,
    -- Why the last look left it open, in a sentence the worker and a person act on.
    held_because TEXT,
    looked_at TIMESTAMPTZ,
    -- What its merge put on the project's timeline.
    timeline_record_id UUID REFERENCES timeline_records (id) ON DELETE SET NULL,
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT work_batch_pull_requests_number_positive CHECK (number > 0),
    CONSTRAINT work_batch_pull_requests_repository_length CHECK (
        char_length(repository) BETWEEN 3 AND 140
    ),
    CONSTRAINT work_batch_pull_requests_url_length CHECK (char_length(url) BETWEEN 1 AND 2048),
    CONSTRAINT work_batch_pull_requests_title_length CHECK (char_length(title) <= 1000),
    CONSTRAINT work_batch_pull_requests_held_because_length CHECK (
        char_length(held_because) BETWEEN 1 AND 4000
    ),
    -- Merged is when, by whom, and nothing holding it; only Elysium's merge has a method.
    CONSTRAINT work_batch_pull_requests_merged_whole CHECK (
        (state = 'merged') = (merged_at IS NOT NULL)
        AND (state = 'merged') = (merged_by IS NOT NULL)
        AND (state <> 'merged' OR held_because IS NULL)
        AND merged_by_elysium = (merge_method IS NOT NULL)
        AND (NOT merged_by_elysium OR state = 'merged')
    )
);

-- A pull request is one batch's: GitHub names repositories ignoring case.
CREATE UNIQUE INDEX work_batch_pull_requests_once
    ON work_batch_pull_requests (lower(repository), number);

CREATE INDEX work_batch_pull_requests_of_batch ON work_batch_pull_requests (work_batch_id);
