-- With one tree and no spaces, a project shows its docs through links: a page may be linked to any
-- number of projects, and a project to any number of pages. See docs/knowledge.md, Pages.
--
-- A link is not a version: it changes nothing a page says, and is recorded in its history alone.
-- It changes nobody's access either: every member reads every page, and no guest reads any.
CREATE TABLE page_projects (
    -- A deleted page keeps its links, so it comes back linked; a project lists live pages alone.
    page_id UUID NOT NULL REFERENCES pages (id) ON DELETE CASCADE,
    -- Deleting a project takes its links out first, each recorded in its page's history
    -- (crate::models::page_project), so the cascade only backs that up.
    project_id UUID NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    -- Who linked it: a person, or the machine user of an agent of Elysium's own. The agent acting
    -- for them is in the page's history.
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL,

    PRIMARY KEY (page_id, project_id)
);

-- Lists a project's pages, and narrows a knowledge search to them.
CREATE INDEX page_projects_project_id_idx ON page_projects (project_id);
