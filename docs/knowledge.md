# Knowledge base

Elysium is its company's knowledge base: a library of everything the company has said, decided, and learned, which
people and their agents search and contribute to. A developer connects Claude Code to the company's Elysium over MCP
and can ask about past meetings, decisions, conversations, and what other agents learned. Everyone in the company
benefits from what anyone's agent saves.

This doc is the design. Little of it is built; the roadmap is the issues in the Knowledge base milestone. Built so
far: the index's tables in Postgres (Search, below), and learnings, saved through the routes, the MCP server, and
Settings, Learnings, filed under the built-in categories and the workspace's own, and indexed as they are saved
(Learnings, below).

## What is indexed

- Everything on the timeline: meetings (corrected transcripts and their segments), mail, Slack, Teams, Confluence,
  Jira, YouTrack, and GitHub issues and comments, and agent turn summaries.
- Learnings that people and agents save.
- The company doc (see `docs/engine.md`).

Repository code is not indexed: agents with a checkout search it directly.

## Who sees what

Everything indexed is visible to everyone in the workspace, a person's own mail, direct messages, and personal chats
included. An instance belongs to one company, whose people are all trusted with its knowledge, and much of what a
company decides is said in mail and chat; a knowledge base without them would miss it. Confidentiality means nothing
leaves the company, and no client's information flows into work for another.

There is no per-owner or per-connector setting. Connecting a mailbox or a chat is what puts its content in the
knowledge base, so every admin and member, and every agent acting for one (Elysia, coding agents, and MCP clients
signed in as a person), can search and read a coworker's indexed mailbox and direct messages, and `knowledge_ask` can
answer from them. Once records are indexed, a mail or chat connector says so where it is added.

Guests are outside the company and outside "everyone". A guest and their agent reach no knowledge base
(`docs/rooms.md`), so the rule reaches a guest in two ways only: what is posted in a room they are in is indexed for
every member, and a member's agent may repeat what it found when it answers a guest in a room, guided by the MCP
server's instructions and the confidentiality rule rather than by a filter.

### The index is shared; records stay their owner's

Two things carry the same text under different rules:

- **The timeline** (`docs/connectors.md`) is each person's feed of what their connectors read. A record belongs to
  whoever it was ingested for: only they list it (`GET /api/v1/timeline`), open it, attribute it to projects, and play
  its recording or clip. Their connectors, with their sign-ins, cursors, checks, and runs, are theirs alone too. This
  ownership decides whose feed a record is on and who controls the reading; it is not a boundary on who may read the
  text.
- **The knowledge index** is the company's: passages made from those records' text, served to every member.

A citation therefore opens in the knowledge base (`knowledge_get`, the Knowledge page), which answers any member for
anything indexed, never through the owner's timeline route, which answers anyone else not found. The index holds
text: a meeting's recording and clips stay with its record and play for its owner alone. Removing a connector stops
its reading; what it recorded stays on its owner's timeline and in the index, since a passage lives as long as its
record.

Action items are one list for the whole workspace (`docs/action-items.md`): an inbox item raised by a
person's mail carries the message's subject, sender, and body, and everyone signed in sees it. Making `owner`
reference users and Next each person's own is on that doc's roadmap; under this rule it decides whose list an item is
on, not who may read it.

### In every query

The rule is applied in the query itself, by leaving the owner out of it: search, `knowledge_get`, `knowledge_ask`,
summaries, and citations carry no per-owner predicate on knowledge-base content. Each passage keeps whose record it
came from (`created_by`), for the `person` filter, for citing who said it, and for removing it with its record. The
`person` filter is a narrowing the caller asks for, never access control.

Access is checked once, on the caller's role: admins, members, and agents acting for them. The knowledge routes and
tools refuse a guest themselves from the first build, rather than waiting on guest rooms, because until roles are
built out a guest has a member's access (`docs/auth.md`).

The learning routes and tools are the first built to this rule: they answer admins and members, and refuse a guest
with `403` and the code `members_only` (`MemberUser` in `api/src/auth/`). No search reads the index yet; each one
is built to the same rule.

## Search

- Postgres alone, with no search cluster: the image is pgvector's build of Postgres 18, pinned by digest
  (`docs/infrastructure.md`). Every source upserts into one document index (`knowledge_documents`), split into
  passages (`knowledge_passages`), each with a full-text vector (exact names, error strings, ticket keys) and an
  embedding (meaning). A search merges both rankings. The tables are in `docs/database.md`.
- Each kind of source is a column of the document, cascading from it, so deleting a source removes its documents and
  passages in Postgres itself, and a document for a source already gone is refused. Indexing a source again upserts
  its one document on that column; the SHA-256 of the text it was indexed from says whether anything changed, and
  passages are rewritten by position, so one whose text held keeps its embedding. The columns today are a timeline
  record's and a learning's; the company doc adds its own.
- Every source enters the index through one function, `knowledge_document::upsert` (`api/src/models/`), and leaves it
  through `remove`. Each takes the caller's transaction, so a source and its document always change together, and
  `KnowledgeSource` is the one place that maps a kind of source to its column. A source's text is cut into passages
  by `knowledge::passages::split` (`api/src/knowledge/`): at paragraph breaks, packed back together up to 1,600
  characters (about 400 tokens), with a longer paragraph cut at the last whitespace that fits. The cut depends on the
  text alone, so the same text always gives the same passages.
- Learnings are indexed in the transaction that saves them. Indexing timeline records, with a backfill, is on the
  roadmap: it maps each record kind to its passages and calls the same upsert.
- The full-text vector uses Postgres's `english` configuration, generated from the passage: words are stemmed and stop
  words dropped on both sides, so a question asked in a sentence matches, while a key such as `WAY-123` stays a
  phrase of its exact tokens. Searches parse their query with `english` too.
- Embeddings come from a small open model, pinned, in its own container on the host, so no text leaves it. Anthropic
  offers no embedding model. The model is to be nomic-embed-text-v1.5 (Apache-2.0, 137M parameters, 8,192-token
  context), small enough for the host's CPU, so the column is `vector(768)`, its size, searched by cosine distance
  through an HNSW index. A passage is `NULL` there until embedded, and each embedding names the model and revision
  that made it, so changing the model re-embeds. Another size is a migration that re-embeds everything. The model
  expects its task prefixes, `search_document: ` on passages and `search_query: ` on queries.
- Embedding is an engine step: new and changed passages are embedded as they arrive, from what the partial index
  `knowledge_passages_unembedded` holds. Until then a passage is found by its words alone.
- The web app has a Knowledge page: search, browse learnings, and ask.

## Learnings

A learning is written for the next agent: its title is the takeaway, its body leads with the rule or decision, then
when it applies and why, and its evidence shows it is true. It records its category, who saved it, and the agent
acting for them. The model is `api/src/models/learning.rs`, the routes `/api/v1/learnings` (`docs/api.md`), and the
tables `learnings` and `learning_events` (`docs/database.md`).

| Field      | Holds                                                                                        |
|------------|----------------------------------------------------------------------------------------------|
| `title`    | The takeaway, one line of up to 300 characters, trimmed and its spacing collapsed             |
| `body`     | The rule or decision first, then when it applies and why; Markdown, up to 20,000 characters   |
| `category` | The key of its category: a built-in one or the workspace's own (Categories, below)          |
| `evidence` | What shows it is true: a command and its output, a link, a quote and who said it, a meeting   |
| `createdBy` | Who first saved it; who changed it since is in its history                                  |
| `agent`    | The agent acting for them: an MCP client, by its id and the name it registered; none when the person saved it themselves |

### Categories

A learning is filed under one category, so a search can filter by it and every agent picks from the same short list.
Every workspace has Elysium's nine built-in categories, always, and adds its own beside them: its subject areas, its
products, its kinds of client work. The built-in nine follow what agents save in practice: the notes an agent keeps in
its own learnings folder (gotchas, setup steps, precedents, where things live, who wants what), and the records a
knowledge base adds (decisions, incidents, project state).

| Category      | What belongs there                                                                  |
|---------------|-------------------------------------------------------------------------------------|
| `decision`    | A choice that was made and why, so it is not argued again                           |
| `convention`  | How work is done here: style, naming, process, review                               |
| `procedure`   | Steps known to work for a task that comes back: setting up, deploying, restoring    |
| `pitfall`     | Something that behaves other than expected, and the way around it                   |
| `reference`   | Facts about a tool, API, or service worth not looking up again: limits, endpoints, flags |
| `environment` | Where things are: hosts, paths, accounts, and how they connect                      |
| `people`      | Who owns, reviews, or decides what, and how they want work delivered                |
| `project`     | A project's or a client's goals, constraints, and current state                     |
| `incident`    | What broke, why, and what fixed it                                                  |

- **One list of rows.** Every category, built-in or the workspace's own, is a row of `learning_categories`
  (`api/src/models/learning_category.rs`), and a learning's `category` references its key, so Postgres itself keeps a
  learning from outliving its category. The nine are seeded by their migration with the descriptions above.
- **A category** is a key, a label, a description, and a place in the order. The key is what a learning stores and an
  agent sends: a lowercase letter, then up to 49 lowercase letters, digits, or underscores, such as `eda_tools`. It
  never changes, since learnings and their history hold it; the label and description can. The description says what
  belongs there, written for the agent choosing a category for what it learned.
- **The order.** The built-in categories come first, in the order above, then the workspace's own in the order its
  admins gave them.
- **Admins manage the workspace's own** under Settings, Learning categories (`/api/v1/learning-categories`,
  `docs/api.md`): add, rename, describe, reorder, and remove. The built-in nine cannot be changed or removed. The list is a workspace setting, so every change is recorded
  in `user_events` with its admin and the values before and after, as the other workspace settings are
  (`learning_category_added`, `learning_category_changed`, `learning_category_removed`).
- **Removing a category moves its learnings first.** A category that learnings are filed under, deleted ones
  included, is removed only once the admin names the category they move to; otherwise the removal answers
  `409 category_in_use`. The web app asks where they move before it removes one. Each move is a revision in that learning's history
  by the admin, in the same transaction as the removal, so no learning is ever left without a category and each move
  shows in its history. Undoing a move would put back a category that no longer exists, so it answers `409`.
- **Agents read the list as it stands.** Whenever a client lists the MCP server's tools, `learning_save`'s schema
  offers every category's key with its label and description, so an agent choosing one sees the workspace's own
  beside the nine. A save naming a key no category has is refused with every key there is.
- **The web app** names and describes the built-in categories from its translations, keyed by their keys, and the
  workspace's own as their admins wrote them. The learning form, the learnings list's category filter, and every
  category chip show both together.

The built-in descriptions are in their seed migration, which is what agents read, and in the web app's translations;
change them together, the seed through a migration of its own.

### Saving revises by title

- A title names one learning, whatever its case or spacing (`learnings_title_unique` on `lower(title)`). Saving a title
  no learning has creates one; saving one a learning has revises it, so an agent corrects a learning by saving its
  title again.
- Saving a deleted learning's title brings it back, revised. The unique title includes deleted learnings, so restoring
  one never meets a live twin.
- A save that changes nothing records nothing and answers `unchanged`.
- A learning is searchable the moment it is saved: the save indexes it in its own transaction (Search, above), as the
  Onyx knowledge-save sidecar's `save_learning` does today. Its document is the title, body, and evidence in that
  order, under the source `learning`, dated when the learning last changed and attributed to whoever first saved it.
  It has no `url`: a learning lives in Elysium, so a citation opens it by id.

### History and undo

Every write records a history entry in the same transaction, shaped as action items' history is
(`docs/action-items.md`, History): who made the change, the agent acting for them, and `data` holding what changed.

| Kind       | `data`                                                                    |
|------------|---------------------------------------------------------------------------|
| `created`  | The learning as first saved: `{ title, body, category, evidence }`          |
| `revised`  | `changes`: each field that changed, `{ from, to }`; `undid` when an undo wrote it |
| `deleted`  | Empty, or `{ undid }`                                                     |
| `restored` | `{ deletedAt }`, the deletion it reversed, and `undid` for an undo         |

- Every revision is kept: a correction, a save of the same title, and an undo are each a `revised` entry.
- Any entry can be undone, by anyone, and the undo is an entry of its own naming the one it reversed (`undid`), so it
  can be undone in turn. A revision is undone field by field: a field that still holds what it wrote gets back what it
  replaced, and one changed again since keeps its newer value, as undoing a changeset does. Undoing the first save or a
  restore deletes the learning; undoing a deletion restores it. A change with nothing left to reverse answers `409`.
- Deleting is soft. A deleted learning leaves the index at once and keeps its history; Restore, or undoing the
  deletion, puts it back in the index.

### Who changes learnings

Anyone in the company may save, correct, delete, restore, and undo any learning, in the browser or through an agent; a
guest reaches none of it (Who sees what, above). Admins see a feed of every change to every learning, newest first
(`GET /api/v1/learnings/feed`), so they follow what people and their agents save. Every change announces
`learning.changed` with the learning's id alone, since a guest hears every event (`docs/realtime.md`).

### In the web app

Settings, Learnings (`/settings/learnings`, in the Knowledge base section) lists the learnings and the deleted ones,
and for admins the feed, each a tab. A learning's page corrects it, deletes or restores it, and lists its history,
each change with Undo. New learning is its own page. Both lists filter by category. Settings, Learning categories
(`/settings/learning-categories`, beside it, for admins) lists every category and manages the workspace's own. The
Knowledge page (Search, above) folds these in once it is built; see `docs/frontend.md`, Learnings and Learning
categories.

## MCP

- Tools return evidence: `knowledge_search` (hybrid search with source, project, person, and date filters, returning
  ranked passages with citations), `knowledge_get` (a whole document or meeting transcript, or a span of it),
  `meetings_list`, `learning_save`, `learning_delete`, `company_doc_read`, and `company_doc_propose`.
- `learning_save` and `learning_delete` are built (`api/src/mcp/learnings.rs`). They call the routes' own `save` and
  `delete` with the connecting person as who saved it and the client as the agent acting for them, so a learning an
  agent saves is validated, recorded, indexed, and announced exactly as one saved in the browser. Until the editable
  instructions below are built, `learning_save`'s description and the server's fixed instructions say what makes a
  learning worth saving.
- `knowledge_ask` runs a search-and-answer agent on Elysium's side and returns a written answer with citations, for
  clients that want an answer rather than evidence. It spends the instance's Claude usage, so it is rate limited per
  person.
- The MCP server sends its instructions to every client that connects: when to search, how to cite, what makes a
  learning worth saving, and the confidentiality rule, including that results may quote a coworker's mail and
  messages and are not for repeating outside the company. Admins edit the text in Settings, versioned, so a change
  reaches every connected agent on its next session. A copyable CLAUDE.md block is offered for teams that want it.

## Onyx

Mooreslab's Onyx knowledge base and its knowledge-save sidecar keep running beside Elysium for now, with agents
writing to both. Retiring it, and whether to import its saved learnings, is decided later. The sidecar files learnings
under a fixed list of its own; an import maps that list onto the built-in categories and the workspace's own.
