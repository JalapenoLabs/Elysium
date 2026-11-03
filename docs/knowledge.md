# Knowledge base

Elysium is its company's knowledge base: a library of everything the company has said, decided, and learned, which
people and their agents search and contribute to. A developer connects Claude Code to the company's Elysium over MCP
and can ask about past meetings, decisions, conversations, and what other agents learned. Everyone in the company
benefits from what anyone's agent saves.

This doc is the design. Little of it is built; the roadmap is the issues in the Knowledge base milestone.

## What is indexed

- Everything on the timeline: meetings (corrected transcripts and their segments), mail, Slack, Teams, Confluence,
  Jira, YouTrack, and GitHub issues and comments, and agent turn summaries.
- Learnings that people and agents save.
- The company doc (see `docs/engine.md`).

Repository code is not indexed: agents with a checkout search it directly.

Everything indexed is visible to everyone in the workspace, the person's own mail and direct messages included. An
instance belongs to one company, whose people are all trusted with its knowledge. Confidentiality means nothing leaves
the company, and no client's information flows into work for another.

## Search

- Postgres alone, with no search cluster: the image becomes pgvector's build of Postgres 18, pinned. Every source
  upserts into one document index, split into passages, each with a full-text vector (exact names, error strings,
  ticket keys) and an embedding (meaning). A search merges both rankings.
- Embeddings come from a small open model, pinned, in its own container on the host, so no text leaves it. Anthropic
  offers no embedding model.
- Indexing is an engine step: new and changed records are embedded as they arrive, and deleting a source removes its
  passages.
- The web app has a Knowledge page: search, browse learnings, and ask.

## Learnings

- A learning is written for the next agent: its title is the takeaway, its body leads with the rule or decision, then
  when it applies, why, and its evidence. It records its category, who saved it, and the agent acting for them.
- A learning is searchable the moment it is saved. Saving the same title again revises it; every revision is kept and
  can be undone. Anyone can correct or delete one, and admins see a feed of new ones.

## MCP

- Tools return evidence: `knowledge_search` (hybrid search with source, project, person, and date filters, returning
  ranked passages with citations), `knowledge_get` (a whole document or meeting transcript, or a span of it),
  `meetings_list`, `learning_save`, `learning_delete`, `company_doc_read`, and `company_doc_propose`.
- `knowledge_ask` runs a search-and-answer agent on Elysium's side and returns a written answer with citations, for
  clients that want an answer rather than evidence. It spends the instance's Claude usage, so it is rate limited per
  person.
- The MCP server sends its instructions to every client that connects: when to search, how to cite, what makes a
  learning worth saving, and the confidentiality rule. Admins edit the text in Settings, versioned, so a change
  reaches every connected agent on its next session. A copyable CLAUDE.md block is offered for teams that want it.

## Onyx

Mooreslab's Onyx knowledge base and its knowledge-save sidecar keep running beside Elysium for now, with agents
writing to both. Retiring it, and whether to import its saved learnings, is decided later.
