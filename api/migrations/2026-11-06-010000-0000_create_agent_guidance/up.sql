-- The guidance every agent connected to Elysium reads as its session starts: MCP clients such as
-- Claude Code and Codex, and every turn of Elysium's own agents. Admins edit it under Settings,
-- Agent guidance. Every version is kept, each by whoever wrote it, and the newest is the one
-- served. What an agent reads is its server's own introduction, then this text, then rules no
-- edit removes (crate::agent_guidance). See docs/knowledge.md, Agent guidance.
CREATE TABLE agent_guidance_versions (
    -- 1 for the seed below, then one more for each save and each restore.
    version INTEGER PRIMARY KEY CHECK (version >= 1),
    -- Markdown, trimmed, and empty when the company has nothing to add to the rules. The API
    -- bounds it tighter, so the whole of what an agent reads fits what Claude Code reads of a
    -- server's instructions (crate::agent_guidance::content_max_characters); this bound only
    -- keeps a row from outgrowing that.
    content TEXT NOT NULL CHECK (char_length(content) <= 4096),
    -- The version a restore copied the text of.
    restored_from INTEGER REFERENCES agent_guidance_versions (version),
    created_by UUID NOT NULL REFERENCES users (id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT agent_guidance_versions_restores_an_earlier_one CHECK (restored_from < version)
);

-- Version 1 is what the MCP server told every client before the guidance could be edited, less
-- the rules Elysium adds to every version itself (crate::agent_guidance::rules), so agents read
-- the same until an admin changes it. Written by the system user.
INSERT INTO agent_guidance_versions (version, content, created_by) VALUES (
    1,
    'Before asking the person something the company may already know, or working out again what '
    'someone may have settled, search the knowledge base with knowledge_search: what was said in '
    'mail, chat, meetings, and pages, and what other agents learned. Open what a search cites with '
    'knowledge_get, and list past meetings with meetings_list. Cite what you use by its title, who '
    'said it, and when.' || E'\n\n' ||
    'The company doc says who the company is and how its people, clients, and terms are spelled: '
    'read it with company_doc_read before writing about them, and propose what you learn for '
    'certain with company_doc_propose, which waits for a person''s approval.' || E'\n\n' ||
    'When you learn something the next agent would otherwise have to rediscover, save it with '
    'learning_save; everyone in the company reads what is saved there, and nothing in it is for '
    'repeating outside the company.',
    '00000000-0000-0000-0000-000000000001'
);
