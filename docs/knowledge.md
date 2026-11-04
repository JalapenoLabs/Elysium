# Knowledge base

Elysium is its company's knowledge base: a library of everything the company has said, decided, and learned, which
people and their agents search and contribute to. A developer connects Claude Code to the company's Elysium over MCP
and can ask about past meetings, decisions, conversations, and what other agents learned. Everyone in the company
benefits from what anyone's agent saves.

This doc is the design. Little of it is built; the roadmap is the issues in the Knowledge base milestone. Built so
far: the index's tables in Postgres (Search, below); every timeline record indexed as it is stored, with the engine's
backfill for what was stored before (Timeline records, below); learnings, saved through the routes, the MCP server, and
Settings, Learnings, filed under the built-in categories and the workspace's own, and indexed as they are saved
(Learnings, below); the company doc, edited in Settings, Company doc, proposed to by its scheduled review and by
agents, read by every agent, and indexed as it changes (The company doc, below); and the embedding model, with the
engine's embedder giving every passage its embedding as it arrives (Embeddings, below).

## What is indexed

- Everything on the timeline: meetings (corrected transcripts and their segments), mail, Slack, Teams, Confluence,
  Jira, YouTrack, and GitHub issues and comments, and agent turn summaries.
- Learnings that people and agents save.
- The company doc, one document per section (The company doc, below).

Repository code is not indexed: agents with a checkout search it directly.

## Who sees what

Everything indexed is visible to everyone in the workspace, a person's own mail, direct messages, and personal chats
included. An instance belongs to one company, whose people are all trusted with its knowledge, and much of what a
company decides is said in mail and chat; a knowledge base without them would miss it. Confidentiality means nothing
leaves the company, and no client's information flows into work for another.

There is no per-owner or per-connector setting. Connecting a mailbox or a chat is what puts its content in the
knowledge base, so every admin and member, and every agent acting for one (Elysia, coding agents, and MCP clients
signed in as a person), can search and read a coworker's indexed mailbox and direct messages, and `knowledge_ask` can
answer from them. A personal connector (mail, chat, or meetings) says so where it is added: its form's help reads
"Everyone in your workspace can search what Outlook reads, in the knowledge base."

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
  record's, a learning's, and a company doc section's.
- Every source enters the index through one function, `knowledge_document::upsert` (`api/src/models/`), and leaves it
  through `remove`. Each takes the caller's transaction, so a source and its document always change together, and
  `KnowledgeSource` is the one place that maps a kind of source to its column. A source's text is cut into passages
  by `knowledge::passages::split` (`api/src/knowledge/`): at paragraph breaks, packed back together up to 1,600
  characters (about 400 tokens), with a longer paragraph cut at the last whitespace that fits. The cut depends on the
  text alone, so the same text always gives the same passages.
- Timeline records, learnings, and the company doc's sections are indexed in the transaction that stores or changes
  them, through the same upsert (Timeline records, below).
- The full-text vector uses Postgres's `english` configuration, generated from the passage: words are stemmed and stop
  words dropped on both sides, so a question asked in a sentence matches, while a key such as `WAY-123` stays a
  phrase of its exact tokens. Searches parse their query with `english` too.
- Embeddings come from a small open model, pinned, in its own container on the host, so no text leaves it. Anthropic
  offers no embedding model. The model is nomic-embed-text-v1.5 (Apache-2.0, 137M parameters), small enough for the
  host's CPU, so the column is `vector(768)`, its size, searched by cosine distance through an HNSW index. A passage
  is `NULL` there until embedded, and each embedding names the model and revision that made it, so changing the model
  re-embeds. Another size is a migration that re-embeds everything. The model expects its task prefixes,
  `search_document: ` on passages and `search_query: ` on queries. See Embeddings, below.
- The engine's embedder embeds new and changed passages as they arrive, from what the partial index
  `knowledge_passages_unembedded` holds. Until then a passage is found by its words alone.
- The web app has a Knowledge page: search, browse learnings, and ask.

## Timeline records

Every timeline record (`docs/connectors.md`, The timeline) is a source of the index: mail, Slack and Teams messages,
Confluence pages and comments, YouTrack issues and comments, meeting segments, and messages a person sent. The code is
`api/src/knowledge/records.rs`, which says what a record's document is, and `timeline_record::index`
(`api/src/models/`), which puts it in the index.

- **On write.** `timeline_record::insert_new`, which every reader, the meetings loop, and sending go through, indexes
  a new record in the same transaction that stores it, so a record is found by its words the moment it commits and a
  record that fails to store leaves nothing indexed. `timelineRecord.created` still carries ids alone; nothing listens
  to it to index. A kind recorded later, such as Jira's or GitHub's issues, is indexed the moment it is on the
  timeline, with nothing added for it.
- **One record, one document.** The index holds a meeting by its segments (each is a record of its own,
  `docs/meetings.md`, Segment records), a conversation by its messages, and a page by its newest version. The
  document's source is the record's kind (`slack`, `meeting`, `confluence`), its title, link, and date the record's,
  and it is attributed to the record's owner (`created_by`), whose timeline it is on.
- **Its text** is the record's title, `From <actor>`, and its body, in that order, cut into passages as every source
  is. A meeting segment names its speakers on every line of its excerpt, so it gives its summary in place of the
  actor. A record with no title, actor, or body has no document.
- **Content, never metadata.** A record's `metadata` and `media` are never indexed: they hold the provider's ids, where
  a reply goes, and where a clip is kept, not what was said. Nothing a connector signs in with is on a record.
- **Size.** Sources keep their own limits: Confluence and sent mail 20,000 characters, Slack 4,000, webmail previews
  and Teams 1,000. A body is indexed up to 100,000 characters (`BODY_MAX_CHARACTERS`), about 63 passages and fifteen
  seconds of the embedding model on four CPUs, which only a long meeting segment's excerpt reaches; past that it is
  cut, so one pathological body cannot hold the embedder for minutes. The record itself keeps all of it.
- **A page's newest version.** Confluence records each version of a page or comment as a record of its own
  (`page:<id>:<version>`), so the timeline shows every edit. The index holds what the page says now: indexing a
  version takes every older version of its connector's page out of the index, and an older version read after a
  newer one, as an overlapping read can, stays out. Versions of one page are indexed under a transaction-level
  advisory lock, so two transactions indexing them at once still leave only the newest. A record whose connector
  was removed stands on its own, since its versions can no longer be told from another site's.
- **Deleted records** leave the index with their document, which cascades. A provider deleting a message or a page is
  not read by any reader today, so it leaves the record, and its document, in place.
- **The same text twice.** Two people whose connectors read the same channel or space each have the record, and the
  index has both documents, with the same `content_sha256`. Search collapses them (on the roadmap with hybrid search).

### The backfill

The engine's backfill (`api/src/knowledge/backfill.rs`) indexes the records stored before records were indexed on
write. Like the embedder it is a loop of the engine's own rather than a workflow, since it has no source of its own
(`docs/engine.md`, Workflows and steps), and it never holds up the engine's start.

- **The walk.** It takes every record in id order, 100 at a time, and indexes each exactly as storing it does, in one
  transaction with its row in `knowledge_backfills` (`docs/database.md`), which records the batch's last id. An engine
  that stops part way resumes after the last batch it committed; a batch cut short is taken again, and indexing a
  record twice changes nothing. A batch shorter than 100 is the end: every record stored since was indexed as it was
  stored.
- **Its pace.** It rests a second between batches, about 360,000 records an hour, so a large timeline never crowds
  the readers' and the API's writes. The embedder takes passages newest documents first, so what was just saved is
  still found by meaning within seconds while the backfill's older passages wait their turn.
- **Once per version.** It walks to `INDEXED_AS` in `records.rs`. A walk that reached the end is never walked again
  until a release changes what a record's text is, or how text is cut into passages, and bumps that version; the
  next engine then walks every record once more, and a record whose text came out the same keeps its passages and
  their embeddings. A release rolled back to an older version leaves the newer walk's text in place.
- **Failures.** A database not answering stops the walk where it is, logged as `knowledge.backfill.failed`, and it is
  tried again 30 seconds later. Its log carries counts, the version, and durations, never a word of a record.

### Not on the timeline yet

- **Jira and GitHub issues and comments** are not read onto the timeline: the link watcher follows linked issues'
  state (`docs/action-items.md`), and no reader records their text. Once one does, they are indexed with nothing more.
- **Agent turn summaries** are kept in `turn_summaries` (`docs/coding.md`, Turn summaries), not on the timeline, so
  they are not indexed yet. Whether they become timeline records of their session's project or a source of the index
  of their own is still to be decided.

## Embeddings

The embedding model turns each passage, and each search's query, into the vector its meaning is searched by. It runs
on the host, so no text leaves it.

### The model and its runtime

- **nomic-embed-text-v1.5** at Hugging Face revision `e5cf08aadaa33385f5990def41f7a23405aec398`: Apache-2.0, 137M
  parameters, 768 dimensions, mean pooling, trained on a context of 2,048 tokens. That revision on purpose: the newest
  (`e9b67630`) has the same weights byte for byte, but its `config.json` names the hidden size twice, which the
  runtime below refuses to read.
- **Hugging Face's text-embeddings-inference (TEI) 1.9.4**, its CPU image, serving the model through ONNX Runtime in
  fp32. TEI is Hugging Face's own server for exactly this: maintained, Apache-2.0, shipped as versioned images that
  pin by digest, with the HTTP API (`/embed`, `/info`, `/health`), batching, tokenization, and truncation built in.
  A server of our own around ONNX Runtime would mean owning all of that.
- **ONNX Runtime, not TEI's Candle backend.** On 4 CPUs of a Ryzen 9 7950X, Candle embedded under 0.4 passages a
  second and took 67 seconds to warm up; ONNX Runtime embeds 4.5 a second and warms up in 16. Both give the same
  vectors (cosine 1.000000 between them on the same text). The image carries the ONNX weights alone, so a
  configuration TEI's ONNX backend cannot read stops it starting, rather than falling back to Candle.
- **fp32, not the int8 model.** The model's int8 ONNX export embeds twice as fast (9.4 a second on the same 4 CPUs),
  but its vectors are only 0.97 cosine from fp32's, which moves rankings. Search quality wins over throughput.
- **2,048 tokens at most.** `--max-batch-tokens 2048` is both a batch's size and the longest input, the context the
  model was trained on, and a longer passage is cut there (`--auto-truncate`). Passages are about 400 tokens, so only
  a rare long one is cut. At TEI's default of 16,384, its warm-up over 8,192 tokens took ONNX Runtime to 7.8 GiB; at
  2,048 it peaks near 1.5 GiB.

### The image

`embeddings/Dockerfile` builds `jalapenolabs/elysium-embeddings`, published with each release like the workspace and
Whisper images (`docs/infrastructure.md`). It is about 2 GB, 550 MB of it the model.

- The base, `ghcr.io/huggingface/text-embeddings-inference:cpu-1.9.4`, is pinned by digest.
- Each of the model's eight files is added with `ADD --checksum`, which refuses a file whose SHA-256 differs and makes
  the checksum the layer's cache key, so a rebuild never downloads it again. They are the first layers after the base.
- It runs as an unprivileged user with the Hub client offline, and serves the model under the name
  `nomic-ai/nomic-embed-text-v1.5@<revision>`, which `/info` answers and which every embedding is recorded as made by
  (`MODEL` in `api/src/embeddings/`; a test holds the two to the same revision).

### A service, not a container per job

Unlike Whisper (`docs/meetings.md`, Transcription), the model runs as a long-lived compose service, `embeddings`
(container `elysium-embeddings`). It is called all day, by the engine for every passage and, once hybrid search is
built, by the API for every search; it holds 1.5 GiB and starts in seconds, and a container per call would pay the
warm-up on every search.

- **Network.** Its only network, `embeddings`, is internal: it has no route out of the host, so nothing sent to the
  model can leave it. Only the engine and the API join it, and reach it at `EMBEDDINGS_URL`
  (`http://embeddings:8080`). nginx does not, so nothing public reaches it. See `docs/security.md`, Embeddings.
- **The API joins it** so hybrid search, on the roadmap, embeds its query directly (`Embeddings::embed_query`)
  rather than asking the engine over the bus: a person is waiting, and the service holds nothing secret.
- **Limits.** Four CPUs, which is also how many threads ONNX Runtime works on, since it counts the container's CPU
  quota, so a host needs at least four. A quarter of Docker's default CPU weight, so the API, Postgres, and agents'
  turns win under contention and a backfill takes what is left. 3 GiB of memory with no swap, 256 processes, a
  read-only root with `/tmp` as a tmpfs, every capability dropped, and no new privileges.
- **Health.** `/health`, with 90 seconds to start. The engine does not wait for it: the embedder tries again until it
  answers.

### The embedder

The engine's embedder (`api/src/knowledge/embedder.rs`) gives every passage its embedding as it arrives. It is a loop
of the engine's own rather than a workflow step: a run is one workflow on one source (`docs/engine.md`, Runs), while
embedding is the same small job for every passage of every source, so a run per document would fill the Runs page
with thousands of one-step runs. Its queue is the index itself.

- **A pass.** It takes up to 32 passages without an embedding, newest documents first, through the partial index
  `knowledge_passages_unembedded`, embeds them in one request with the document prefix, and records each embedding
  with its `embedding_model`, then takes the next batch, until none waits. Newest first, so what was just saved is
  found by meaning within a pass even during a backfill. No connection or lock is held while the model works.
- **Rewritten meanwhile.** An embedding is recorded only while its passage's text is still what was embedded:
  indexing a source again may rewrite a passage during the pass, and the new text waits for an embedding of its own.
- **When.** Between passes it looks every 10 seconds, an index scan of an empty partial index when nothing waits, so
  a passage saved now is found by its words at once and by meaning within seconds.
- **Another model.** As the engine starts, every embedding another model made is forgotten (`embedding` and
  `embedding_model` set to `NULL`), so a release that changes the model re-embeds every passage through the same
  queue. The look is an index scan of `knowledge_passages_embedding_model`, for the models before and after the
  current one, so it stays cheap however many passages the timeline brings. Vectors of two models are not comparable, so meanwhile those passages are found by their words alone.
- **Checked before it writes.** Once a pass, when there is work, it asks the service's `/info` which model it serves.
  A service serving another, such as a deployment whose images drifted apart, embeds nothing, and each pass logs it
  at error level (`knowledge.embeddings.wrong_model`). Every vector is checked to be 768 finite numbers.
- **Failures.** A service that is starting, stopped, or refusing, or a database not answering, ends the pass, and the
  next is tried 30 seconds later; what earlier batches recorded stays. The service going away is a warning once and
  coming back an info line, not a line each retry. A shutdown loses at most the batch in flight, which is taken
  again.
- **No activity.** A batch takes seconds and is safe to repeat, so the embedder holds no activity for auto-update.
- **The log** carries counts and durations (`knowledge.embeddings.stored`), never a word of a passage.

### How fast it is

`embeddings/measure.sh` runs the image under the service's limits, under `nice` and with no network, embeds every
passage of the checkout's `docs/*.md`, cut about as the knowledge base cuts text, 32 at a time with the document prefix,
and prints the host, the rate, and the peak memory.

| Host                                             | CPUs | Passages | Rate | Peak memory |
|--------------------------------------------------|------|----------|------|-------------|
| Development: AMD Ryzen 9 7950X (16 cores, 32 threads), 125 GiB, kernel 7.2.8, 2026-11-04 | 4 | 554, 1,256 characters on average | 4.5 a second, 16,254 an hour | 1.3 GiB |
| Production VM, n2-highcpu-32 (32 vCPUs)          | 4    | Measured at deploy | Measured at deploy | Measured at deploy |

To measure the production VM after a deploy that built `elysium-embeddings:local`, from the deployment's checkout:

```sh
embeddings/measure.sh
```

`--cpus 8` (or any other count) measures another cap, to judge whether the service's four are worth raising.

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

## The company doc

One document per instance knows the company. Every agent reads it, and correcting a meeting's transcript leans on it
most (`docs/engine.md`, Meetings), so it carries how the company spells its people's names and its jargon. The model
is `api/src/models/company_doc.rs`, what every caller shares of it `api/src/company_doc/`, the routes
`/api/v1/company-doc` (`docs/api.md`), and the tables `company_doc_sections`, `company_doc_proposals`,
`company_doc_events`, and `company_doc_review_cursor` (`docs/database.md`).

### Sections

The doc has seven fixed sections, each Markdown of at most 12,000 characters, in this order:

| Section        | What belongs there                                                                        |
|----------------|-------------------------------------------------------------------------------------------|
| `company`      | Who the company is: what it does, for whom, and where                                    |
| `leadership`   | Who leads it, and what each of them owns                                                 |
| `people`       | Everyone who comes up, one per line: their name as they spell it, how it is often misheard, and what they do |
| `stakeholders` | Clients, partners, investors, and vendors, with who speaks for each                      |
| `products`     | What the company makes and sells                                                         |
| `projects`     | The work under way, and who is on it                                                     |
| `glossary`     | Names, acronyms, and jargon, one per line in alphabetical order, as the company spells them |

The sections are rows seeded by their migration; people never add or remove one. The limit keeps the whole doc
inside the review's one prompt beside what it reads (`docs/engine.md`, The company doc). The API names and describes
each section to agents in English; the web app names and describes them from its translations.

### Who changes it

- **People edit it directly**, any number of sections at once from the page's Save, as one change in its history.
- **Everyone else proposes.** The scheduled review, Elysia, coding agents, and people's MCP clients never write the
  doc: each proposes a change to one section, and nothing changes until a person approves it. Any member approves or
  rejects any proposal, one at a time; a guest reaches none of it (Who sees what, above).

Proposals are the company doc's own, not changesets (`docs/action-items.md`, Changesets). A changeset is a batch of
action-item operations, scoped to a project, with dependencies between them, applied as its proposer through the
action-item writes and reviewed in the Action items area. A proposal to the doc is one section's text, with no
dependencies, reviewed beside the section it changes. Folding it into changesets would carry project scoping and the
action-item review into a document that has neither, so the doc keeps the rule (nothing applies until a person
approves each change, and every applied change is recorded and can be undone) in a model of its own.

### Proposals

- **A proposal** is one section's new text with the text it was made against, its reason, and optionally the quote
  and source it was read in (and, for the review's, the timeline record and the run). It is `pending` until a person
  approves or rejects it, and records who decided and when.
- **Agents propose edits**, in the form Claude Code's Edit tool takes: `find` is text the section holds exactly once,
  and `replace` takes its place; an empty `find` adds to the end. Elysium applies them to the section as it stands and
  stores the result, so an agent that misreads a long section cannot drop what it did not mean to change. An edit
  whose text is missing or found twice is refused, saying which.
- **The same change is pending once.** Proposing a section text that is already pending answers that proposal, so a
  review that runs twice, or two agents reading the same message, make one.
- **Approving** writes the proposal into its section as a change by the person approving. When the section changed
  since the proposal was made, the two changes are merged three ways from the text it was made against, line by
  line, the way version control merges a branch; a proposal whose lines were changed since is refused with
  `409 proposal_overlaps` and stays pending, for the person to reject or make by hand. Without the merge, every edit
  to People or the glossary would leave every pending proposal to them unusable.
- **Rejecting** changes nothing in the doc and is not part of its history; the proposal keeps the decision, and the
  review reads the latest rejections so it does not propose them again.

Who proposed is recorded as learnings record who saved (`created_by` and `agent`): a person and the MCP client acting
for them (`mcp:<client id>`), the coding agent's machine user for a session (`session:<number>`), or Elysia, for her
turns and for the review, which also names its run.

### History and undo

Every change that writes the doc records a history entry in the same transaction, shaped as learnings' history is:

| Kind       | `data`                                                                                   |
|------------|------------------------------------------------------------------------------------------|
| `edited`   | `changes`: each section that changed, `{ from, to }`; `undid` when an undo wrote it      |
| `approved` | `changes`, as for an edit; the entry names the proposal it applied (`proposal_id`)       |

- An entry names who made the change: the person who edited, approved, or undid.
- Any entry can be undone by any member, and the undo is an `edited` entry naming the one it reversed, so it can be
  undone in turn. It is undone section by section: a section that still holds what the entry wrote gets back what it
  replaced, and one changed since keeps its newer text. A change with nothing left to reverse answers `409`.
- A section given its own text again is not part of a change, and a save that changes nothing records nothing.

### The review

A workflow on the engine, `company_doc_review`, reads what was ingested since its last run and proposes changes, as
Elysia. Its one step, `review_company_doc`, is a text turn on the step's model setting, Opus 5.5 at high effort unless
an admin chose another. Its schedule, its window, and how it fits one prompt are in `docs/engine.md`, The company doc.

### In the index

Each section is a document of its own (`company_doc`, titled "Company doc: Glossary" and so on), upserted in the
transaction that changes it, so search finds what the doc says the moment it commits and a citation names the section.
An empty section leaves the index. A section's document is attributed to the person whose change it now reads as.

### Agents read it

- Over MCP, `company_doc_read` answers every section, in order, with what belongs there and the proposals still
  pending, so an agent knows what may change and need not propose it again; `company_doc_propose` proposes, needing
  `workspace:write`, as the person and their client (`docs/mcp.md`).
- In a turn, the same two tools are on the `elysium_knowledge` server, which every coding and Studio session's turn
  and every one of Elysia's gets, project or not (`docs/coding.md`). A session proposes as its coding agent, Elysia as
  herself.
- Every change announces `companyDoc.changed`, with nothing in it, and members load the doc again
  (`docs/realtime.md`).

### In the web app

Settings, Company doc (`/settings/company-doc`, in the Knowledge base section, for members) edits every section in
one form saved from the header, each section with its pending proposals beside it as a diff with Approve and Reject,
and the history below with Undo. Look for updates starts a review at once. See `docs/frontend.md`, Company doc.

## MCP

- Tools return evidence: `knowledge_search` (hybrid search with source, project, person, and date filters, returning
  ranked passages with citations), `knowledge_get` (a whole document or meeting transcript, or a span of it),
  `meetings_list`, `learning_save`, `learning_delete`, `company_doc_read`, and `company_doc_propose`.
- `company_doc_read` and `company_doc_propose` are built (`api/src/mcp/company_doc.rs`; The company doc, above).
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
