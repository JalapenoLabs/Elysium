# Knowledge base

Elysium is its company's knowledge base: a library of everything the company has said, decided, and learned, which
people and their agents search and contribute to. A developer connects Claude Code to the company's Elysium over MCP
and can ask about past meetings, decisions, conversations, and what other agents learned. Everyone in the company
benefits from what anyone's agent saves.

This doc is the design. Little of it is built; the roadmap is the issues in the Knowledge base milestone. Built so
far: the index's tables in Postgres (Search, below); every timeline record indexed as it is stored, with the engine's
backfill for what was stored before, agent turn summaries among them (Timeline records, below); learnings, saved through the routes, the MCP server, and
the Knowledge page, filed under the built-in categories and the workspace's own, and indexed as they are saved
(Learnings, below); hybrid search, by words and by meaning, with its filters, and opening a document whole, on the
Knowledge page and in the topbar's search (Hybrid search, below); the company doc, edited and reviewed on its pages in the
knowledge base, suggested to by its scheduled review and by agents, read by every agent, and indexed as it changes (The
company doc, below); the embedding model, with the engine's embedder giving every passage its embedding as it arrives
(Embeddings, below); the tools that search, open, and list the knowledge base for MCP clients and for every agent's
turn (MCP, below); asking it for a written answer, every claim citing a passage, on the Knowledge page and over MCP
(Asking, below); the guidance every connected agent reads as its session starts, edited by admins in Settings,
Agent guidance, every version kept (Agent guidance, below); and pages, their tree, every published version, their labels,
their place in the index, finding them by title and text, and the edits agents suggest to them, reviewed on the page,
with learnings and the company doc among them (Pages, below).

## What is indexed

- Everything on the timeline: meetings (corrected transcripts and their segments), mail, Slack, Teams, Confluence,
  Jira, YouTrack, and GitHub issues and comments, and agent turn summaries.
- Learnings that people and agents save.
- The company doc, one document per section (The company doc, below).
- Every other page, one document per page, reading as its newest version (Pages, below).

Repository code is not indexed: agents with a checkout search it directly.

## Who sees what

Everything indexed is visible to everyone in the workspace, a person's own mail, direct messages, and personal chats
included. An instance belongs to one company, whose people are all trusted with its knowledge, and much of what a
company decides is said in mail and chat; a knowledge base without them would miss it. Confidentiality means nothing
leaves the company, and no client's information flows into work for another.

There is no per-owner or per-connector setting. Connecting a mailbox or a chat is what puts its content in the
knowledge base, so every admin and member, and every agent acting for one (Elysia, coding agents, every project's
Project Manager, and MCP clients signed in as a person), can search and read a coworker's indexed mailbox and direct messages, and `knowledge_ask` can
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

The learning routes and tools, and the knowledge routes and tools (`/api/v1/knowledge`: the search, a document, and the
meetings), are built to this rule: they answer admins and members, and refuse a guest with `403` and the code
`members_only` (`MemberUser` in `api/src/auth/`), as the agent guidance's routes do. None carries a per-owner predicate;
the `person` filter is a parameter like the others. An agent's turn reads for whoever started its session, checked on
every call, so a session a guest started reaches none of it; Elysia's turns read as herself (MCP, below).

## Search

- Postgres alone, with no search cluster: the image is pgvector's build of Postgres 18, pinned by digest
  (`docs/infrastructure.md`). Every source upserts into one document index (`knowledge_documents`), split into
  passages (`knowledge_passages`), each with a full-text vector (exact names, error strings, ticket keys) and an
  embedding (meaning). A search merges both rankings. The tables are in `docs/database.md`.
- Each kind of source is a column of the document, cascading from it, so deleting a source removes its documents and
  passages in Postgres itself, and a document for a source already gone is refused. Indexing a source again upserts
  its one document on that column; the SHA-256 of the text it was indexed from says whether anything changed, and
  passages are rewritten by position, so one whose text held keeps its embedding. The columns today are a timeline
  record's and a page's; a learning and a company doc section are pages, told apart by their document's `source`.
- Every source enters the index through one function, `knowledge_document::upsert` (`api/src/models/`), and leaves it
  through `remove`. Each takes the caller's transaction, so a source and its document always change together, and
  `KnowledgeSource` is the one place that maps a kind of source to its column. A source's text is cut into passages
  by `knowledge::passages::split` (`api/src/knowledge/`): at paragraph breaks, packed back together up to 1,600
  characters (about 400 tokens), with a longer paragraph cut at the last whitespace that fits. The cut depends on the
  text alone, so the same text always gives the same passages.
- Timeline records, learnings, the company doc's sections, and pages are indexed in the transaction that stores or
  changes them, through the same upsert (Timeline records, below).
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
- The web app has a Knowledge area (`/knowledge`, for members): pages, search, ask (Asking, below), and browse learnings. Its search and each document are in `docs/frontend.md`, Knowledge; the topbar's search lists the
  knowledge base's documents beside its other results for members.

## Hybrid search

`KnowledgeSearch::search` (`api/src/knowledge/search.rs`) is the one search of the index. `GET
/api/v1/knowledge/search` answers it (`docs/api.md`, Knowledge), and the MCP server's `knowledge_search` calls the same
function, so every caller finds the same passages in the same order. `knowledge_document::get` opens one document
whole (`GET /api/v1/knowledge/documents/{id}`), as every citation does.

### Two rankings, fused

- **Words.** The query is parsed with `websearch_to_tsquery('english', ...)`, the parser and stemmer the passages'
  `search_vector` is generated with, so both sides cut text alike: `WAY-123` is the phrase `'way' <-> '-123'` on both,
  and "what is WAY-123 about?" loses its stop words to the same phrase. It never fails on what was typed, and reads
  quotes as a phrase, `or` as either, and a leading `-` as without, as a search box does. Every word left must match.
  Matches rank by `ts_rank_cd`, which favours passages whose matched words sit close together. A key typed without its
  dash (`WAY 123`) is the words `way` and `123`, which a passage holding `WAY-123` does not have (its `-123` is a
  signed number to the parser); meaning still finds it, ranked lower.
- **Meaning.** The query is embedded with the `search_query: ` prefix and the nearest passages are read by cosine
  distance through the HNSW index, `hnsw.ef_search` raised to the 100 asked for and `hnsw.iterative_scan` on, so a
  filter that leaves out most neighbours still answers in full. Only passages within 0.45 of the query count
  (`MAX_MEANING_DISTANCE`, a cosine similarity of 0.55): the nearest passages are always some distance away, and on
  realistic mail, chat, pages, meeting segments, and learnings, related passages lay 0.23 to 0.40 from their query
  and unrelated ones 0.42 to 0.53. Past it a passage can still be found by its words. The distance belongs to the
  model; another model is measured again.
- **Fusion.** Each ranking takes its closest 100 passages under the same filters, and the two lists are merged by
  Reciprocal Rank Fusion (Cormack, Clarke, and Büttcher, 2009): a passage scores `1 / (60 + rank)` in each list it is
  in, summed. RRF reads ranks alone, so `ts_rank_cd`'s unbounded numbers and cosine distances never need weighing
  against each other, and a passage one ranking cannot see (not embedded yet, or sharing no word with the query)
  still scores from the other. It is the method Elasticsearch and Azure AI Search ship for merging lexical and vector
  results, and 60 is the constant it was published and validated with. A weighted sum of normalized scores was the
  alternative, which needs its weights tuned to the score distributions of one model and one corpus.
- **Documents.** A document ranks by its best passage, so a long one is not favoured for matching often, and shows
  up to three of its passages, best first. Ties read newest first.

### The same text twice

Two people whose connectors read the same channel or space each have the record (Timeline records, The same text
twice), so the index holds two documents of one source with the same `content_sha256`. Their passages are the same
text at the same positions and rank alike, so a result folds them: the earliest recorded is cited, and the others are
listed as also recorded by their people. A document's own page lists its copies, found through
`knowledge_documents_copies`. Pages are not folded: two pages can say the same thing, such as two branches' empty
Overview pages, and each is its own place in the tree.

### Filters

Every filter applies to both rankings, inside the query, and narrows; none is access control.

| Filter    | Keeps                                                                                            |
|-----------|--------------------------------------------------------------------------------------------------|
| `source`  | Documents of these sources (`slack`, `outlook`, `meeting`, `learning`, `company_doc`, `page`, and the rest) |
| `project` | Timeline records attributed to it, at any confidence, and pages linked to it, learnings and the company doc's sections among them |
| `person`  | Documents whose record, learning, section edit, or newest version came from this person (`created_by`) |
| `label`   | Pages carrying this label, however it was typed (Pages, Labels); nothing else has labels     |
| `from`, `to` | What was said from one moment and before the other, with offsets                             |

### When meaning is not available

A query the embedding model cannot embed within 4 seconds (the service is starting, stopped, or busy) is searched by
its words alone, and the answer says so (`searchedByMeaning: false`). A passage not embedded yet is found by its words
alone. Nothing about the model ever fails a search. The service going away is logged once at warn
(`knowledge.search.meaning_unavailable`) and its return once at info (`knowledge.search.meaning_restored`); each
search logs its counts and durations at debug (`knowledge.search.answered`), never the query or a passage.

### Budget and cost

- Each person may search 20 times at once, refilling 2 a second (`PER_PERSON`), however they search: the route, their
  MCP clients' `knowledge_search`, and their sessions' agents all spend the same budget through
  `KnowledgeSearch::admit`, and Elysia's turns spend one of her own. Past it, `429` with the code
  `knowledge_search_rate_limited`, which a tool answers as its message. It is lower than the global search's titles
  because every search embeds its query on the service the embedder shares.
- On the development host a query embeds in about 15ms when the service is idle. Over 30,000 passages, the meaning
  ranking read the HNSW index in 8ms, and a filter keeping a third of them was answered exactly, by a scan, in 11ms;
  a word matching a fifth of the passages ranked in 20ms. A word matching a large share of a much larger index ranks
  every match before taking the best, which grows with the index; a ranked full-text index (RUM) is the answer if it
  is ever slow.

## Timeline records

Every timeline record (`docs/connectors.md`, The timeline) is a source of the index: mail, Slack and Teams messages,
Confluence pages and comments, YouTrack issues and comments, meeting segments, messages a person sent, and what
Elysium's agents put on a project's timeline, a Project Manager's turns and batches and a session's turn summaries. The
code is
`api/src/knowledge/records.rs`, which says what a record's document is, and `timeline_record::index`
(`api/src/models/`), which puts it in the index.

- **On write.** `timeline_record::insert_new`, which every reader, segmenting, and sending go through, indexes
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
  index has both documents, with the same `content_sha256`. Search folds them into one result (Hybrid search, The same
  text twice).

### Turn summaries

Every coding and Studio session's turn summary (`docs/coding.md`, Turn summaries) of a session with a project is a
timeline record of that project, kind `turn_summary` (`api/src/sessions/timeline.rs`), so a project's timeline reads its
agents' work beside its meetings and messages, and a search finds what an agent did.

- **Written with its summary.** The record is stored, attributed to the session's project as certain, and indexed in
  the transaction that records the summary, so a summary is never kept without its record.
- **One per turn.** The record names its summary (`turn_summary_session_id`, `turn_summary_turn_id`, unique), so
  writing it again, as a replaced summary or the backfill does, leaves one record that reads as the summary does now,
  and deleting the session deletes it with the summary, as the session's other history goes.
- **The workspace's.** Every session is listed to everyone, so its summaries are not its starter's feed: the record is
  owned by the coding agent's machine user, and every company member reads it on the project's timeline, as they read a
  Project Manager's records (`timeline_record::SHARED_OWNERS`). A guest reads only their own records, as for every
  shared record. In the index it is attributed to that machine user, so the `person` filter names no person for it.
- **Its text.** The title is the summary, the actor the session (`Session 12`), and the body empty, so its document is
  the summary and `From Session 12`. Its metadata names the session, the turn, and the Studio item when the session is
  one's, which the timeline opens the session's conversation at.
- **A session without a project**, which only a Studio session can be, keeps its summaries, and they go on no
  timeline and into no index: a record on no project would read as news on every member's whole timeline.

### The backfill

The engine's backfill (`api/src/knowledge/backfill.rs`) brings into the index what was stored before it was indexed
on write: the timeline records stored before records were indexed, and the turn summaries written before they went on
the timeline. Like the embedder it is a loop of the engine's own rather than a workflow, since it has no source of its
own (`docs/engine.md`, Workflows and steps), and it never holds up the engine's start.

- **Its sources.** It walks each in turn, each with its own row in `knowledge_backfills` (`docs/database.md`):
  timeline records, each indexed exactly as storing it does, then turn summaries, each put on its session's project's
  timeline exactly as writing it does, which indexes the record. A summary of a session without a project is walked
  past.
- **The walk.** It takes a source's rows in id order (a summary's by its turn id), 100 at a time, and brings each in,
  in one transaction with the source's row, which records the batch's last id. An engine that stops part way resumes
  after the last batch it committed; a batch cut short is taken again, and bringing a row in twice changes nothing. A
  batch shorter than 100 is the end: every row stored since was brought in as it was stored.
- **Its pace.** It rests a second between batches, about 360,000 rows an hour, so a large timeline never crowds the
  readers' and the API's writes. The embedder takes passages newest documents first, so what was just saved is still
  found by meaning within seconds while the backfill's older passages wait their turn.
- **Once per version.** Each source walks to its own version: records to `INDEXED_AS` in `records.rs`, summaries to
  `RECORDED_AS` in `api/src/sessions/timeline.rs`. A walk that reached the end is never walked again until a release
  changes what it writes (a record's text, how text is cut into passages, what a summary's record holds) and bumps
  that version; the next engine then walks every row of it once more, and a record whose text came out the same keeps
  its passages and their embeddings. A release rolled back to an older version leaves the newer walk's text in place.
- **Failures.** A database not answering stops the walk where it is, logged as `knowledge.backfill.failed`, and it is
  tried again 30 seconds later, from the first source not yet walked. Its log names the source and carries counts,
  the version, and durations, never a word of a record.

### Not on the timeline yet

- **Jira and GitHub issues and comments** are not read onto the timeline: the link watcher follows linked issues'
  state (`docs/action-items.md`), and no reader records their text. Once one does, they are indexed with nothing more.

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
(container `elysium-embeddings`). It is called all day, by the engine for every passage and by the API for every
search; it holds 1.5 GiB and starts in seconds, and a container per call would pay the
warm-up on every search.

- **Network.** Its only network, `embeddings`, is internal: it has no route out of the host, so nothing sent to the
  model can leave it. Only the engine and the API join it, and reach it at `EMBEDDINGS_URL`
  (`http://embeddings:8080`). nginx does not, so nothing public reaches it. See `docs/security.md`, Embeddings.
- **The API joins it** so a search embeds its query directly (`Embeddings::embed_query`) rather than asking the
  engine over the bus: a person is waiting, and the service holds nothing secret. A query waits at most 4 seconds
  (`QUERY_TIMEOUT`), then is searched by its words alone (Hybrid search, When meaning is not available).
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

- **A pass.** It takes up to 8 passages without an embedding, newest documents first, through the partial index
  `knowledge_passages_unembedded`, embeds them in one request with the document prefix, and records each embedding
  with its `embedding_model`, then takes the next batch, until none waits. Newest first, so what was just saved is
  found by meaning within a pass even during a backfill. No connection or lock is held while the model works.
- **Rewritten meanwhile.** An embedding is recorded only while its passage's text is still what was embedded:
  indexing a source again may rewrite a passage during the pass, and the new text waits for an embedding of its own.
- **Small batches.** The service answers requests in the order they arrive, so a search's query waits behind the
  batch in flight. Behind 32 passages it waited 5.6 seconds on 4 CPUs of the development host; behind 8, about one
  (two on the production VM). Eight cost nothing: the service batches the model's work by its own token budget
  whatever a request holds, and 8 at a time embedded 5.8 passages a second against 5.3 for 32, since shorter batches
  pad less.
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
| Production VM: Intel Xeon @ 2.80GHz (n2-highcpu-32, 32 vCPUs), 31 GiB, kernel 7.2.14, 2026-11-04 | 4 | 585, 1,255 characters on average | 3.7 a second, 13,350 an hour | 1.5 GiB |
| Production VM, the same host with `--cpus 8` | 8 | 585, 1,255 characters on average | 6.8 a second, 24,560 an hour | 1.5 GiB |

To measure the production VM after a deploy that built `elysium-embeddings:local`, from the deployment's checkout:

```sh
embeddings/measure.sh
```

`--cpus 8` (or any other count) measures another cap, to judge whether the service's four are worth raising.

## Learnings

A learning is written for the next agent: its title is the takeaway, its body leads with the rule or decision, then
when it applies and why, and its evidence shows it is true. It records its category, who saved it, and the agent
acting for them. A learning is a page with a category and evidence (Pages, Learnings and the company doc, below). The
rules here are `api/src/models/learning.rs` and the routes `/api/v1/learnings` (`docs/api.md`).

| Field      | Holds                                                                                        |
|------------|----------------------------------------------------------------------------------------------|
| `title`    | The takeaway, one line of up to 300 characters, trimmed and its spacing collapsed             |
| `body`     | The rule or decision first, then when it applies and why; Markdown, 1 to 20,000 characters   |
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
  (`api/src/models/learning_category.rs`), and a learning's page references its key (`pages.category`), so Postgres
  itself keeps a learning from outliving its category. The nine are seeded by their migration with the descriptions
  above.
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
  shows in its history. Undoing a move would put back a category that no longer exists, so it answers `409`. Restoring a
  version filed under a removed category brings back what it says under the category the learning has now, and the
  version keeps recording the one it held.
- **Agents read the list as it stands.** Whenever a client lists the MCP server's tools, `learning_save`'s schema
  offers every category's key with its label and description, so an agent choosing one sees the workspace's own
  beside the nine. A save naming a key no category has is refused with every key there is.
- **The web app** names and describes the built-in categories from its translations, keyed by their keys, and the
  workspace's own as their admins wrote them. The learning form, the learnings list's category filter, and every
  category chip show both together.

The built-in descriptions are in their seed migration, which is what agents read, and in the web app's translations;
change them together, the seed through a migration of its own.

### Saving revises by title

- A title names one learning, whatever its case or spacing (`pages_learning_title_unique` on `lower(title)`, for pages
  that are learnings). Saving a title no learning has creates one, under the Learnings page; saving one a learning has
  revises it, wherever in the tree it sits, so an agent corrects a learning by saving its title again.
- Saving a deleted learning's title brings it back, revised. The unique title includes deleted learnings, so restoring
  one never meets a live twin. One deleted along with a page above it comes back once that page has.
- A save that changes nothing records nothing and answers `unchanged`.
- A learning is searchable the moment it is saved: the save indexes it in its own transaction (Search, above), as the
  Onyx knowledge-save sidecar's `save_learning` does today. Its document is the title, body, and evidence in that
  order, under the source `learning`, dated when its newest version was published and attributed to who published it,
  as every page's is. It has no `url`: a learning lives in Elysium, so a citation opens it by id.

### History and undo

Every write records a history entry in the same transaction, on its page (Pages, History). A learning's history reads
as action items' does (`docs/action-items.md`, History): who made the change, the agent acting for them, and `data`
holding what changed. `crate::models::learning` reads it from the page's entries and versions, so a `revised` entry's
`changes` are always what lies between two versions, and a page's moves are not part of it.

| Kind       | `data`                                                                    |
|------------|---------------------------------------------------------------------------|
| `created`  | The learning as first saved: `{ title, body, category, evidence }`          |
| `revised`  | `changes`: each field that changed, `{ from, to }`; `undid` when an undo wrote it, and `restoredFrom` when a page's version was restored |
| `deleted`  | Empty, or `{ undid }`                                                     |
| `restored` | `{ deletedAt }`, the deletion it reversed, and `undid` for an undo         |

- Every revision is kept as a version: a correction, a save of the same title, and an undo are each a `revised` entry.
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

The Knowledge area's Learnings (`/knowledge/learnings`) lists the learnings and the deleted ones, and for admins
the feed, each a tab of its own. A learning reads as a page, with its category and evidence after its text, and its
Edit corrects it, deletes or restores it, and lists its history, each change with Undo. New learning is its own page. Both lists filter by category. Settings, Learning categories
(`/settings/learning-categories`, in the Knowledge base section, for admins) lists every category and manages the
workspace's own; it is a setting, so it stays in Settings. See `docs/frontend.md`, Learnings and Learning categories.

## The company doc

One document per instance knows the company. Every agent reads it, and correcting a meeting's transcript leans on it
most (`docs/engine.md`, Meetings), so it carries how the company spells its people's names and its jargon. Each
section is a page under the Company page (Pages, Learnings and the company doc, below). The model is
`api/src/models/company_doc.rs`, what every caller shares of it `api/src/company_doc/`, the route that starts its review
`POST /api/v1/company-doc/review` (`docs/api.md`), and the table `company_doc_review_cursor` (`docs/database.md`).
Everything else about it is its pages': their versions, their history, and the edits suggested to them (Pages,
Suggestions, below).

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

The sections are pages seeded by their migration; nobody adds, removes, moves, or renames one. The limit keeps the whole doc
inside the review's one prompt beside what it reads (`docs/engine.md`, The company doc). The API names and describes
each section to agents in English; the web app names and describes them from its translations.

### Who changes it

- **People edit it directly**, each section on its own page, as they edit any page.
- **Everyone else suggests.** The scheduled review, Elysia, coding agents, Project Managers, and people's MCP clients
  never write the doc: its sections are proposal-only pages (Pages, Who changes pages), so each suggests an edit to one
  section, and nothing changes until a person accepts it. Any member accepts or dismisses any suggestion, on the
  section's own page; a guest reaches none of it (Who sees what, above).

A suggestion to the doc is a suggestion to its section's page, through the framework every page shares (Pages,
Suggestions, below): the doc adds no rule of its own to how one is made, reviewed, merged, or recorded.
`company_doc_propose` is a name for `page_suggest` that names a section by its key rather than its page, and suggests
against the section as it reads now, so the agents and guidance that use it keep working.

Suggestions are not changesets (`docs/action-items.md`, Changesets). A changeset is a batch of action-item operations,
scoped to a project, with dependencies between them, applied as its proposer through the action-item writes and
reviewed in the Action items area. A suggestion is one page's text, with no dependencies, reviewed beside the page it
changes. Folding it into changesets would carry project scoping and the action-item review into pages that have
neither, so pages keep the rule (nothing applies until a person accepts each change, and every applied change is a
version that can be restored) in a model of their own.

### History

The doc keeps no history of its own: each section's is its page's (Pages, History). Every change is a version of the
section it reached, with who made it and the agent acting for them, read, compared, and restored from that page's
History tab, and a version an accepted suggestion wrote names the suggestion. A change that reached several sections at
once is one entry id with a row on each section's page.

### The review

A workflow on the engine, `company_doc_review`, reads what was ingested since its last run and suggests edits, as
Elysia, reading the suggestions waiting and the latest dismissed with why, so it does not suggest them again. Its one step, `review_company_doc`, is a text turn on the step's model setting, Opus 5.5 at high effort unless
an admin chose another. Its schedule, its window, and how it fits one prompt are in `docs/engine.md`, The company doc.

### In the index

Each section is a document of its own (`company_doc`, titled "Company doc: Glossary" and so on), upserted in the
transaction that changes it, so search finds what the doc says the moment it commits and a citation names the section.
Its text is the section's alone, without its page's title. An empty section leaves the index. A section's document is
attributed to the person whose change it now reads as.

### Agents read it

- Over MCP, `company_doc_read` answers every section, in order, with what belongs there and the suggestions still
  pending, as `proposals`, so an agent knows what may change and need not suggest it again;
  `company_doc_propose` suggests, needing `workspace:write`, as the person and their client (`docs/mcp.md`).
- In a turn, the same two tools are on the `elysium_knowledge` server, which every coding and Studio session's turn,
  every one of Elysia's, and every Project Manager's gets, project or not (`docs/coding.md`). A session suggests as
  whoever started it, naming the session, Elysia as herself, and a Project Manager as itself
  (`project_manager:<project id>`), as `page_suggest` records them.
- Every change to a section announces `page.changed` for its page (`docs/realtime.md`).

### In the web app

The company doc lives in the knowledge base, nowhere else. The Company page lists its seven sections as its child
pages; each is read, edited, compared, and restored as any page is, and its suggestions are reviewed on its own
Suggestions tab (Pages, In the web app, below). Look for updates, an action on the Company page and on every section,
starts a review at once and says so in a toast linking to its run. `/settings/company-doc` opens the Company page.

## MCP

Tools return evidence, and the calling agent reasons with it. External clients reach them on the MCP server
(`api/src/mcp/`, `docs/mcp.md`); every agent's turn reaches the reading ones, the company doc's, and the pages' on the
`elysium_knowledge` server at `/api/agent-tools/elysium_knowledge` (`api/src/tools/knowledge.rs`, `docs/coding.md`),
under the same names, descriptions, and argument schemas.

| Tool                  | MCP | Turn | Does                                                                  |
|-----------------------|-----|------|-----------------------------------------------------------------------|
| `knowledge_search`    | Yes | Yes  | Hybrid search with its filters: citations and their best passages     |
| `knowledge_get`       | Yes | Yes  | One document a citation names, whole or a span of its passages        |
| `meetings_list`       | Yes | Yes  | The meetings the index holds, newest first, with their topics         |
| `knowledge_ask`       | Yes | No   | A written answer to a question, every claim citing a passage (Asking, below) |
| `knowledge_ask_get`   | Yes | No   | An ask by its id, waiting a bounded time for its answer               |
| `company_doc_read`    | Yes | Yes  | The company doc (The company doc, above)                              |
| `company_doc_propose` | Yes | Yes  | `page_suggest` for one section, named by its key                      |
| `learning_save`       | Yes | No   | Saves or revises a learning (Learnings, above)                        |
| `learning_delete`     | Yes | No   | Deletes a learning softly                                             |
| `page_read`           | Yes | Yes  | One page whole, with its `currentVersion` and the oldest 20 suggestions waiting |
| `page_list`           | Yes | Yes  | One branch of the tree, 200 pages at a time, each with its `childCount` |
| `page_save`           | Yes | Yes  | Creates a page or publishes its next version (Who changes pages, below) |
| `page_suggest`        | Yes | Yes  | Suggests an edit to a page, for a person to accept (Suggestions, below) |
| `page_move`           | Yes | Yes  | Moves a page with everything under it, or reorders it                 |
| `page_label_add`      | Yes | Yes  | Puts a label on a page (Labels, below)                                |
| `page_label_remove`   | Yes | Yes  | Takes a label off a page                                              |
| `page_project_link`   | Yes | Yes  | Links a page to a project (Projects, below)                           |
| `page_project_unlink` | Yes | Yes  | Takes a page off a project                                            |

- **One function per route.** `knowledge_search`, `knowledge_get`, and `meetings_list` call the functions the
  knowledge routes call (`search`, `open`, and `list_meetings` in `api/src/routes/v1/knowledge/`), with the same
  argument structs, whose doc comments are the arguments' descriptions. A turn's schemas are the MCP server's, made by
  rmcp's own generator, so the two never drift.
- **Who reads.** Over MCP, the person who connected the client, checked with `MemberUser` on every call. In a turn,
  whoever started the session, read from the database on every call: a company member passes, and so does one of
  Elysium's own machines, while a guest or a person disabled since is refused with a message saying the knowledge base
  is for the company's members alone. Elysia's turns read as herself. The company doc's tools in a turn
  are checked the same way.
- **Citations open in the knowledge base.** Every result is a `Citation`, `documentId` first, which `knowledge_get`
  opens, never the owner's timeline route. The MCP server's instructions tell a client where a person opens one:
  `https://<host>/knowledge/documents/<documentId>`, the Knowledge page's document.
- **A span.** A long document, such as a meeting topic's excerpt, is read in parts: a search answers each passage's
  position, `knowledge_get` takes `fromPosition` and `toPosition` (both inclusive), and `passageCount` says how many
  there are in all. The route takes the same two as query parameters.
- **Meetings.** A meeting is in the index as its topics, each a segment record's document (`docs/meetings.md`,
  Segment records). `meetings_list` (`GET /api/v1/knowledge/meetings`, `api/src/models/knowledge_meeting.rs`) gathers
  those documents back into their meetings by the `meetingId`, `meetingTitle`, and `meetingSource` every segment
  record carries, so a meeting's transcript is read topic by topic with `knowledge_get`. It answers each meeting's
  title, source, when its first topic began, whose source recorded it, and its topics in the order they were said,
  filtered by `from` and `to` (when it started), `person`, and `project` (a topic attributed to it), 20 unless asked
  for up to 50. The `meetings` row itself stays its owner's and is never read for it. Two people whose sources
  recorded one meeting have a meeting each, segmented apart, and both are listed.
- **Descriptions teach searching.** `knowledge_search`'s description says what is indexed, that exact keys and error
  strings are found by their words and paraphrases by their meaning, that quotes keep a phrase, that filters only
  narrow and `person` is whoever said it, what `searchedByMeaning: false` means, to cite by title, person, and date,
  and that results may quote a coworker's mail and are not for repeating outside the company.
- **Pages.** `page_read`, `page_save`, `page_suggest`, `page_move`, `page_label_add`, `page_label_remove`,
  `page_project_link`, and `page_project_unlink` call what the page routes call, with the argument structs beside those routes, so a turn's schemas are the MCP server's as the knowledge tools'
  are. `page_list` answers one branch, from what `GET /api/v1/pages` reads: every learning is a page, so the flat list
  soon passes what a tool may answer (Claude Code refuses about 25,000 tokens, some 300 pages), and an agent wants to see
  how the tree is laid out, not all of it at once. It starts at the top and follows `childCount`; to find what a page
  says it searches. Every learning sits under one page, so one branch can pass that limit too: an answer holds at most
  200 pages, `total` says how many the branch has, and `offset` reads on.
- `learning_save` and `learning_delete` call the routes' own `save` and `delete` with the connecting person as who
  saved it and the client as the agent acting for them, so a learning an agent saves is validated, recorded, indexed,
  and announced exactly as one saved in the browser. `learning_save`'s description and the agent guidance (Agent
  guidance, below) say what makes a learning worth saving. A turn cannot save learnings yet: an agent acting for a
  session needs recording as one, as `session:<n>` is for a suggestion, which is on the roadmap.
- `knowledge_ask` answers in writing rather than with evidence, for clients that want an answer (Asking, below). A
  turn does not get it: an agent searches for itself, and an answer would spend a second turn on what the first one
  can read.
- Every client that connects to the MCP server, and every turn that connects to `elysium_knowledge`, is told the
  company's agent guidance: when to search, how to cite, what makes a learning worth saving, and the confidentiality
  rule, which no edit removes (Agent guidance, below).

## Agent guidance

Every agent connected to Elysium reads the company's guidance as its session starts: MCP clients such as Claude Code
and Codex as they connect to `/api/mcp`, and every turn of Elysium's own agents (coding and Studio sessions, and
Elysia) as it connects to `elysium_knowledge`. Admins edit it under Settings, Agent guidance, and every version is
kept. The code is `api/src/agent_guidance/`, the table `agent_guidance_versions` (`docs/database.md`), and the routes
`/api/v1/agent-guidance` (`docs/api.md`).

### What an agent reads

Three paragraphs, in this order (`instructions` in `api/src/agent_guidance/`):

| Part                         | From                                   | Says                                                  |
|------------------------------|----------------------------------------|-------------------------------------------------------|
| Its server's introduction    | Compiled, one per kind of agent        | What the server is; for a turn, that it has no `learning_save` |
| The guidance                 | The newest version admins wrote        | When to search, how to cite, what is worth saving, when to write a page rather than a learning, and whatever else the company asks |
| The rules                    | Compiled, part of no version           | Results may quote a coworker's mail and messages and never leave the company; a person opens a citation at `https://<host>/knowledge/documents/<documentId>` |

- **Fixed rules, not a check.** The confidentiality rule and the citation link follow every version, compiled, rather
  than being part of the text with a check that refuses an edit without them. A check can only look for exact words,
  so it would refuse a rule reworded to say the same and accept one reworded to say less, and the link names this
  deployment's origin, which is not an admin's to type. An admin may write anything, and nothing they write removes the
  rules. The page shows them below the editor, as every agent reads them.
- **Within what Claude Code reads.** Claude Code reads at most 4,096 characters of a server's instructions and drops
  the rest (`CLAUDE_CODE_MAX_MCP_DESCRIPTION_LENGTH`), so the whole of what an agent reads stays within that: the
  guidance may be as long as what is left once the longer introduction and the rules are counted, about 3,400
  characters, which the routes answer as `maxCharacters`. A longer save is refused, so the rules at the end are never
  cut off.
- **A turn has no `learning_save`.** The guidance is written for every agent, so a turn's introduction says it has no
  `learning_save` and asks for what was learned in the turn's answer instead.
- **What a tool does belongs to the code.** How to use a tool goes in its description, or in its server's compiled
  introduction, never in the guidance: the guidance is the company's to edit, and a release cannot change it without
  overriding an admin. A new tool, such as `knowledge_ask`, describes itself.
- **The default.** Written by the system user in two versions: search the knowledge base before asking or working
  something out again, open and cite what it finds by title, person, and date, read the company doc before writing
  about its people, clients, and terms and propose what is certain, save as a learning what the next agent would
  otherwise rediscover, and write a page rather than a learning for what people read as a whole and keep up to date.
- **A release never changes what an admin wrote.** A new default is a migration adding a version, and it adds one only
  while the versions a release wrote are the only ones. A deployment whose admins saved or restored anything keeps
  exactly what they chose, and they add the new words themselves if they want them. Rolling it back takes that version
  away only if nothing was written after it.

### Versions

- **Never changed.** Saving adds the next version, and restoring an earlier one adds the next version with its text,
  naming the one it copied (`restoredFrom`). The versions are the history, each with who wrote it and when, and a
  restore is undone by restoring the version before it.
- **Made against a version.** Every save names the version it was edited from (`baseVersion`). A save against one that
  is no longer current is refused with `409 guidance_changed`, so an admin never replaces a version they have not seen.
  The number is the primary key, so two saves at once cannot both take the next one.
- Text the current version already holds records nothing, and neither does restoring a version whose text is current.
- **Who.** Admins save and restore. The company's members read every version and copy the `CLAUDE.md` block, so a
  developer adds it to a repository without asking an admin; a guest reaches none of it. Every new version announces
  `agentGuidance.changed`, with nothing in it.

### On every session

- **Read as it connects.** Both servers are stateless, so each session starts with an `initialize` (or, on newer
  protocol versions, a `discover`) answered on its own, and both read the current version then. A change reaches every
  connected agent on its next session; a session already running keeps what it read, since a client reads
  instructions once.
- **Kept in memory.** The API keeps the newest version: read from Postgres on the first session after it starts,
  replaced by every save and restore as it commits, and never by an older one, so a read that began before a save
  cannot bring back the text it replaced. A session's start costs no query. The API runs one replica, and this is
  listed among what holds it to one (`docs/engine.md`, One replica of the API).
- **Unreadable.** A session that starts while Postgres cannot be read fails to initialize, logged as
  `mcp.guidance_unreadable` or `tools.endpoint.guidance_unreadable`, rather than run on text without the guidance.

### The CLAUDE.md block

Teams that want the guidance in their repositories copy a block made from the current version: a comment naming its
version and where to copy it again, an `## Elysium` heading, this deployment's `claude mcp add` command, the guidance,
and the rules. An agent working in the repository reads them whether or not it is connected, and a team can tell from
the version when its copy is behind. Nothing updates a copy; a team copies a newer version again.

### In the web app

Settings, Agent guidance (`/settings/agent-guidance`, in the Knowledge base section, for members) edits the guidance
for admins, saved from the header, with the rules shown below it, the `CLAUDE.md` block with Copy, and every version
with Restore. Members read it all and copy the block. See `docs/frontend.md`, Agent guidance.

## Asking

`knowledge_ask` answers a question in writing, every claim citing the passages that say it, for people on the
Knowledge area's Ask and for MCP clients that want an answer rather than evidence. The pure parts (the prompt, the
checks on the answer, and each person's budget) are `api/src/knowledge/ask.rs`; the routes `/api/v1/knowledge/asks`
(`docs/api.md`); the engine's answerer `api/src/knowledge/answerer.rs`; the table `knowledge_asks`
(`docs/database.md`).

### Retrieve, then answer

- **The search first.** Asking searches the knowledge base as the asker, with the question as the query and the
  filters they named, through the one search (Hybrid search, above), so it spends one of their searches and finds what
  they would. It takes 10 documents with up to three passages each (`ASK_DOCUMENTS`), and the ask keeps what the search
  offered by reference alone, `{ documentId, position }` in order: a passage's text stays in the index and leaves with
  its record.
- **Nothing found, no turn.** A search that offers nothing answers the ask at once with nothing to say, and no model
  is called.
- **One text turn.** Otherwise the engine's answerer writes the answer in one text turn on the runner
  (`TurnMode::Text`: no tools, no settings, no MCP servers), whose prompt is the question and the passages, each
  labelled `P1`, `P2`, and so on with its source, title, person, and date, and which answers in JSON: claims, each
  citing the labels that say it. Retrieving first was chosen over letting the model call `knowledge_search` and
  `knowledge_get` itself: one model call with everything it may cite in front of it answers in seconds rather than
  over several tool round trips, needs no tool grant (grants are scoped to a session, which an ask has none of), and
  its citations can be checked against exactly what it was shown. The passages are told to it as evidence, never
  instructions, and a passage cannot close its own block.
- **The prompt's size.** Ten documents of three passages are about 48,000 characters; the prompt stops adding
  passages at 120,000 bytes, so an answer costs what a page of results costs, and a passage left out is offered to
  nobody.
- **The model.** The `knowledge_ask` agent's, resolved as each answer starts (`docs/engine.md`, Models). It is a
  triage agent: like deciding what a message asks for, it reads what was said across the workspace for no one
  project, and a person acts on what it says, so it runs on the kind's Opus 5.5 at high effort, which cites
  faithfully, unless an admin chooses another on its own row of Settings, Models.

### Every claim cites a passage

Nothing the model answers is trusted (`check_answer`):

- A citation counts only when it names a label the prompt offered. The labels are Elysium's, never the index's ids,
  so a model cannot cite what it was not shown by guessing one, and a passage removed from the index since the search
  is offered to nobody.
- A claim keeps only the citations that count. One left with none is not part of the answer, and the answer counts it
  (`unsupportedClaims`), so the person knows something was left out.
- The sources are the passages the claims cite, numbered in the order the answer first cites them, each its
  `Citation` and the position of the passage, which `knowledge_get` and the document's page open.
- An answer that is not the asked-for JSON fails the ask, saying so; at most 30 claims of 1,500 characters are kept.

### Budget

An answer spends the instance's Claude usage, which every agent shares, so each person may ask 20 questions in any
hour (`ASKS_PER_WINDOW`), with 3 waiting for an answer at once (`WAITING_PER_PERSON`); past either, `429
knowledge_ask_rate_limited`, saying when to ask again. The count is of the asks recorded, under a lock of the asker's,
so asks made at once count each other and a restart forgets nothing; the search's budget is in memory and per
second, and cannot hold an hour.

### Waiting for the answer

- **The engine answers.** The API runs no background work (`docs/engine.md`), so asking records the ask and signals
  `knowledgeAsks`; the answerer takes queued asks oldest first, three at once at most, so asks never take more than
  three of the runner's eight turn slots. It is a loop of the engine's own rather than a workflow: a run is shown to
  every member on the Runs page, while an ask is its asker's alone, answered in seconds, with nothing to retry, skip,
  or resume; one that fails says why and is asked again.
- **Announced.** An ask is announced as it is made, taken, and ended (`knowledgeAsk.changed`, its id, asker, and
  state alone), and the asker's page loads it by id.
- **Over MCP.** A tool call is one request under the 30 seconds every request has, so `knowledge_ask` waits up to 20
  seconds for the answer and otherwise answers the ask `queued` or `running`, and `knowledge_ask_get` waits for it again,
  never asking twice (`docs/mcp.md`).
- **Bounded.** An answer not written within three minutes is cancelled and fails. An answer whose credentials are
  spent fails saying Claude's usage is spent until it resets.
- **No activity of its own.** The runner holds one for every turn, so auto-update waits for an answer being written;
  loading the passages and checking the answer take milliseconds. An engine that stops kills the turn and the ask fails
  saying so; one left running by an engine that died is failed as the next starts.

### Whose asks

An ask is its asker's alone: only they list it, read it, and wait on it, and anyone else's is not found. What it cites
is the company's, as every search result is (Who sees what, above). The log names an ask by its id and counts its
claims, never the question or a passage.

## Pages

Everything authored is one kind: a page. People and their agents read and write pages the way they use Confluence, in
one tree for the whole workspace. The model is `api/src/models/page.rs`, the routes `/api/v1/pages` (`docs/api.md`),
and the tables `pages`, `page_versions`, `page_events`, and `page_projects` (`docs/database.md`), and the Knowledge area
reads them (In the web app, below). The rest of the design (Roadmap, below) is the issues in the Knowledge base milestone.

### The tree

- **One tree.** Pages nest under one tree, to any depth, with no spaces. A page at the top has no parent. Moving a page
  takes everything under it along.
- **Three rules hold it**, and the API keeps them: a live page's parent is live, a page is never under itself at any
  depth, and siblings never tie. A moved page's new parent is checked against everything under the page, which only
  holds if two moves at once cannot each pass alone, so every change to the tree's shape (creating, moving, deleting,
  restoring) takes one advisory lock. People change the tree's shape rarely enough that this costs nothing. A publish
  changes no shape and takes only its page's row.
- **Order.** A page's `position` is a number with gaps, 2^20 apart as they are appended: a page lands at the midpoint
  of its new neighbours, writing its own row alone, so reordering never rewrites its siblings. A spot runs out of room
  after about twenty pages are dropped into it, and then that parent's children are spread a gap apart again in one
  statement, each announced. Fractional text keys were the alternative, which never renumber, but need their own
  midpoint arithmetic and a byte-order collation for a case this rare. Clients name a place by the sibling a page
  follows and never compute a position; they sort by it.
- **Deleting is soft and takes what is under a page.** Each page taken leaves the index and keeps its versions and
  place. Restoring a page brings it back where it was, with the ones its deletion took, which share its `deleted_at`;
  a child deleted on its own beforehand stays deleted. A page whose parent is deleted cannot be restored until its
  parent is. A deleted page is read like any other and changes no further.
- **Or a page goes alone.** Its live children then take its place among its siblings, in their order, each with
  everything under it, in the one transaction that deletes it, so nothing can see a tree between the two. They never
  leave the index. Restoring the page moves them back to the positions they left, which nothing can have taken, since a
  deleted page's children change no further; a child moved or deleted since stays where someone put it.

### Versions

- **Publish is the only write of a page's content.** Creating a page publishes version 1. Each publish is the next
  version, with who made it and the agent acting for them, and `pages` keeps a copy of the newest so the tree and title
  searches read one table. A version is never changed once written, and any version reads back whole.
- **Made against a version.** Every publish names the version it was edited from (`baseVersion`). A publish against one
  that is no longer current is refused with `409 page_changed`, so nobody overwrites a version they have not seen, and
  the row lock and the version's key mean two publishes at once cannot both land.
- **The same text is no version.** Text the page already holds records nothing and answers `unchanged`, whatever
  version it names, since it would overwrite nothing: a page opened and published as it was, or two people publishing
  the same words. The comparison is after normalizing: a title is trimmed and its spacing collapsed, and a body's line
  endings become `\n` and its trailing whitespace goes. Nothing else about its Markdown is touched, and an editor that
  writes a page back exactly apart from those has changed nothing.
- **Restoring.** Any version can be restored: its content is published as the next version, naming the one it copied
  (`restoredFrom`), so the versions in between stay and restoring one of them puts it back. A restore names no base
  version, since what it replaces stays a version of its own; a version whose content the page already holds records
  nothing.
- **Limits.** A title is 1 to 300 characters on one line, and a body up to 100,000, which is what the index takes from
  one source (Timeline records, Size), so everything stored is searchable. A body may be empty: a page can exist only
  to hold others.

### History

Every change records an entry in `page_events`, in the transaction that makes it, shaped as learnings' history is:
who made it, the agent acting for them, and `data`.

| Kind        | `data`                                                                                  |
|-------------|-----------------------------------------------------------------------------------------|
| `created`   | `{ parentId, version }`                                                                 |
| `published` | `{ version }`, with `restoredFrom` for a restore, `undid` for an undo of a learning or of the company doc, and `suggestionId` for an accepted suggestion |
| `moved`     | `{ from, to }`, each `{ parentId, after }`: the two values a move takes                 |
| `deleted`   | Empty, or `{ deletedWith }`, the ancestor whose deletion took it; `undid` for an undo; `{ keptChildren }`, each `{ id, position }`, when it went alone |
| `restored`  | `{ deletedAt }`, the deletion it reversed, with `restoredWith` when an ancestor's restore brought it back; `undid` for an undo |
| `labeled`   | `{ label }`                                                                             |
| `unlabeled` | `{ label }`                                                                             |
| `attached`  | `{ attachmentId, name }`, for a file added or put back                                  |
| `detached`  | `{ attachmentId, name }`, for a file taken off                                          |
| `project_linked` | `{ projectId, projectName }`, the name as it was then, since an entry outlives a project deleted later |
| `project_unlinked` | `{ projectId, projectName }`, by hand or because the project was deleted                   |

Versions hold what a page said, and the history everything that happened to it, publishes included, so one table reads
as a page's history and as a feed across every page, in order by id. A later kind of change adds a kind, with no
migration. A page has no undo of its own: a version is undone by restoring the one before it, a deletion by restoring
the page, a move by moving it back to its `from`, a label by putting it back, and a file taken off by restoring it. Learnings and the company doc keep the
field-by-field undo they promise (Learnings and the company doc, below).

An entry is named by its page and its id. One change that reaches several pages is one id with a row on each, which is
how a save of several company doc sections stays one change with one Undo, and how a child's move reads as part of the
deletion that left it in its parent's place, or the restore that took it back.

### Who changes pages

- **Admins and members** read and edit every page, in any way; guests reach none (Who sees what, above). Every route
  refuses a guest with `403 members_only` (`MemberUser`).
- **Agents write as their person, or as themselves.** Every write takes an `Author`: who it is by and the agent acting
  for them (`mcp:<client id>`, `session:<number>`, or `project_manager:<project id>`). Each route's work is a function
  of its own taking one, so an agent's tool calls what the route calls. Agents edit pages directly, since every version
  is kept and can be restored, or suggest an edit for a person to accept (Suggestions, below); what an agent should not
  do alone is a page that says so (below).

  | Agent                       | Reads as                     | Writes                                               |
  |-----------------------------|------------------------------|------------------------------------------------------|
  | An MCP client               | The person who connected it  | With `workspace:write`, as them, naming the client   |
  | A coding or Studio session  | Whoever started the session  | As them, naming the session                          |
  | A Project Manager           | Its machine user             | As its machine user, naming its project              |
  | Elysia                      | Herself                      | As herself, with no agent to name                    |
  | A pool worker               | The coding agent             | Nothing: it reports to its Project Manager           |

  A person's agent is checked as the person is on every call, so a guest's client, or a session a guest started or
  whose starter was disabled since, is refused. A worker runs unattended for a batch rather than a person, no recorded
  form names it, and what it learned is its Project Manager's to write up, from its result.
- **`page_save` is both routes.** An agent has one tool, so it takes one flat set of arguments
  (`api/src/routes/v1/pages/save_page.rs`). With `id` and `baseVersion` it publishes; a stale one is refused with
  `page_changed`, which says to read the page again and save against what that answers, and never carries the version
  itself, so an agent cannot retry without reading what it would overwrite. Without them it creates, and takes
  `parentId`, `null` for the top of the tree: required, as a move's is, so nothing lands there by accident, and open to
  agents, since a fresh tree holds only the Learnings, Company, and Project briefs pages. Anything else is refused by
  name.
- **Learnings are `learning_save`'s.** `page_save` refuses a learning, and a new page directly under Learnings, with
  `409 page_is_a_learning`. In the browser a person edits one as any page, but an agent's edit would rewrite a rule
  under the evidence for the rule it replaced, and a page it created under Learnings would be an ordinary page, never
  listed as a learning. `learning_save` takes the evidence with the text and revises by title. A turn has no
  `learning_save`, so it is told to say what should change in its answer. Moving a learning is only a move, so
  `page_move` takes one.
- **Proposal-only pages.** A page may be `proposalOnly`: people edit it directly, and an agent may only suggest.
  Anything but a person acting alone is refused, publishing, restoring, moving, or deleting it, or deleting a page
  above it, with `409 page_proposal_only`, whose message names `page_suggest`. Who that is is read from the user
  as well as the agent named, so Elysia, who writes as her machine user with no agent, is refused too, and a caller that
  forgets to name its agent is refused all the same. A client that cannot be recorded as an agent is refused any change
  to a page first, since it would read as the person alone. No route sets it: the Company page and the doc's sections
  are the only ones that have it.
- **Fixed pages.** The Learnings page, the Company page, the doc's seven sections, and the Project briefs page are ones
  Elysium relies on, each at an id fixed in code, and so is each project's brief while its project exists (Projects,
  below). They keep their place: nobody moves, deletes, or renames one, people included, with
  `409 page_fixed`. What a fixed page says is written like any page's, under its rule above. Other pages may sit
  beside and under them.
- Every change announces `page.changed` for each page it touched, with the page's id alone, since a guest hears every
  event (`docs/realtime.md`).

### Suggestions

Any agent may suggest an edit to any page, for a person to accept: Elysia, Project Managers, coding and Studio
sessions, and people's MCP clients, with `page_suggest`. It is one framework for every page, the company doc's sections
among them, so a suggestion is made, reviewed, merged, and recorded the same way on any page. The model is
`api/src/models/page_suggestion.rs`, the routes `/api/v1/pages/{id}/suggestions` (`docs/api.md`), and the table
`page_suggestions` (`docs/database.md`).

- **A suggestion** is the text a page would hold, the version it was written from with that version's text, its
  reason, and optionally the quote and source it was read in (and, for the company doc's review, the timeline record
  and the run). It changes nothing until a person accepts it. It is `pending` until a person accepts or dismisses it,
  and keeps who decided, when, and, for a dismissal, why.
- **Agents suggest edits** in the form Claude Code's Edit tool takes: `find` is text the version holds exactly once,
  and `replace` takes its place; an empty `find` adds to the end. `page_suggest` names the version the agent read
  (`baseVersion`, as `page_save` does) and the edits apply to that version, so an agent that misread a long page cannot
  drop what it did not mean to change, and one that read an older version suggests against what it saw rather than
  being refused. An edit whose text is missing or found twice is refused, saying which.
- **What it changes.** A page's text alone: its title, place, labels, links, and files are each changed their own way
  (above). A suggestion fits the page's bounds, 100,000 characters, or a section's 12,000.
- **The same change is pending once** on a page: suggesting text already pending answers that suggestion, so a review
  that runs twice, or two agents reading the same message, make one.
- **Not on a learning, nor a deleted page.** A learning is changed through `learning_save`, with its evidence, so
  `page_suggest` refuses one with `409 page_is_a_learning`, as `page_save` does. A deleted page changes no further
  (`409 page_deleted`). It keeps the suggestions that waited on it, as it keeps its labels and files: they can be
  dismissed, its inbox item stays until they are decided or a person closes it, and restoring the page brings them
  back to be accepted.
- **Who suggested** is recorded as every write of a page records its author (Who changes pages, above): an MCP
  client's person, naming the client; a session's starter, naming the session; a Project Manager's machine user,
  naming its project; and Elysia as herself, for her turns and for the review, which also names its run.

#### Reviewing one

- **Its changes.** A suggestion is read as its changes: each run of lines it adds, removes, or replaces against the
  version it was written from, in order, with a few unchanged lines around each (`page_suggestion::changes`, on the
  lines as `similar` diffs them). The edits an agent sent are not kept, since each later edit's `find` reads the text
  the earlier ones left.
- **Accepting**, wholly or in part. A person takes every change, or picks which, and the ones left out are no part of
  what is written. The chosen changes are applied to the version the suggestion was written from, then merged three
  ways with what the page holds now, line by line, the way version control merges a branch, and published as the
  page's next version by the person accepting, its history naming the suggestion (`suggestionId`). A change whose lines
  were changed since is refused with `409 suggestion_overlaps` and the suggestion stays pending, for the person to take
  the rest, dismiss it, or make the change by hand; each change says whether it still merges, so the page shows which
  before anyone presses Accept. Without the merge, every edit to a busy page (People, the glossary) would leave every
  suggestion to it unusable. Changes the page already holds are accepted without a version. The suggestion keeps which
  changes were taken (`acceptedChanges`, `null` for all of them).
- **Dismissing** takes a reason, 1 to 2,000 characters, changes nothing on the page, and is no part of its history.
  The company doc's review reads the latest dismissals with their reasons, so it does not suggest them again.
- **Who decides.** Whoever may edit the page: today every admin and member, on every page, the company doc's sections
  included, since a person edits a proposal-only page directly. A person decides alone, in the browser
  (`MemberUser`): no agent's tool accepts or dismisses, so no agent ever accepts its own suggestion. A guest reaches
  none of it.

#### Telling people

- **Once per page.** While a page has suggestions waiting, one inbox item asks for them to be reviewed, titled with how
  many wait and the page's title ("Review 2 suggested edits to Glossary"), its notes naming the page's Suggestions tab
  as where they are reviewed, as a project's approval item names the work queue. It is kept up to date as suggestions arrive and are decided, in the transaction that changes them, and resolved once
  none are left (`page_suggestion::review_item`, `page_suggestion_items`), as a project's approval item is for its
  proposed batches (`docs/engine.md`, Proposed batches). Elysium itself raises it (`system`), whoever suggested, so it
  never reads as a person's own. A person who resolves or dismisses it by hand is done with it: deciding the rest
  changes nothing, and the next suggestion raises a new one. The inbox is everyone's one list, so a page's title
  reaches it as a message's subject does (Who sees what, above); what a suggestion says stays on the page.
- **In the tree and in search.** `GET /api/v1/pages` counts each page's waiting suggestions, and so do the topbar's
  page results and the knowledge search's results for a page, so a page with suggestions waiting says so wherever it
  is listed.
- Every suggestion made, accepted, or dismissed announces `page.changed` for its page, with its id alone.

### Labels

Any page takes any number of labels, which cut across the tree. The model is `api/src/models/page_label.rs`, the table
`page_labels`, and the routes `POST /{id}/labels` and `DELETE /{id}/labels/{label}`.

- **One spelling.** A label is stored lowercase with each run of spaces made one hyphen, so `On Call` and `on-call` are
  one label, whichever way it is written or searched for. It is 1 to 50 characters of letters and digits of any
  alphabet, hyphens, underscores, and dots, and starts with a letter or digit. Anything else is refused with `422` and
  what a label may be, rather than cleaned up: a slash or a run of dots would change the address it is removed at.
- **At most 50 on a page.** Nothing about labels needs a limit that low to work. It bounds what an agent's loop can pile
  on, and no page reads well past it.
- **A label is not content.** Adding or removing one makes no version, moves no `updated_at`, and is no part of what a
  version restores, so restoring an old version keeps the page's labels as they are. It is an entry in the page's
  history, in the same transaction, naming who did it and the agent acting for them, and it announces `page.changed`. A
  label already there, or one the page does not carry, records nothing.
- **Who labels.** Everyone who may write a page: people, and agents acting for them or as themselves with
  `page_label_add` and `page_label_remove`, through a function taking an `Author` (`page_label::add` and `remove`) as
  every write of a page does. They are tools of their own rather than part of `page_save`, which refuses the pages a
  label may go on. A proposal-only page keeps what it says from agents, and a fixed page keeps its place and name; a
  label changes neither, and labelling is what `docs/vision.md` starts in the tier that needs no approval, so both take
  labels from anyone, and the history says who. Only a deleted page refuses, with `409 page_deleted`, as it changes no
  further; it keeps its labels and comes back with them.
- **A learning's category is not a label.** It is a filed, fixed choice with rules of its own. A learning is a page, so
  it can also carry labels, which stay out of its own history and feed, as a move does.
- **In the index, a label is metadata read through the page.** Nothing about it is copied to `knowledge_documents` or
  into a passage: the search's `label` filter and a document's `labels` read `page_labels` through the document's page,
  as the `project` filter reads a record's attributions. So a label reaches the index in the transaction that writes it,
  and no change of labels can touch a document's text, its SHA-256, or an embedding. The cost is that a label's words
  are not searched: `runbook` in a search box finds pages that say it, and the filter finds pages that carry it.

### Attachments

Files and images attach to a page. The rules for what a page takes and how a file is served are `api/src/page_attachments.rs`,
the rows are `api/src/models/page_attachment.rs` and the table `page_attachments`, and the routes are under
`/api/v1/pages/{id}/attachments`, with what every page shares at `/api/v1/page-attachments`.

- **Where they go.** One storage location, an admin's choice (Settings, Storage, Page attachments), saved like the
  backups' destination: the location is reached before it is chosen, and the change is recorded with the workspace's
  settings. Until one is chosen, nothing can be attached, and every page says so before anyone tries, naming who can fix
  it. Each row keeps the location its file went to, so changing the choice moves nothing, and a location stays while it
  is chosen or holds a file. Files sit under `page-attachments/<page>/<attachment>/<name>` in its directory, and do not
  count toward its storage limit, as backups do not: that limit is what Studio holds itself to (`docs/storage.md`).
- **An attachment is not content.** It makes no version and moves no `updated_at`, as a label does not, so a page
  stays as it was. It is an entry in the page's history, and it announces `page.changed`, whose loaded page lists it.
- **In the text it is plain Markdown.** The rich editor puts a reference where a file was dropped: `![name](address)` for
  an image and `[name](address)` for anything else. The address is
  `/api/v1/pages/<page>/attachments/<attachment>/<name>`, with its name in plain letters and hyphens so nothing needs
  encoding, and a person or agent reading the source reads what it is. The id finds the file, and the last segment is for
  whoever reads it. Each row of a page's list copies its Markdown, for a file attached without the editor.
- **Limits are constants.** A file is at most 50 MB and a page holds at most 200, and `GET /api/v1/page-attachments`
  serves the limits, so the web app refuses a file before it uploads, in its own words. The API refuses again,
  whichever way a request arrives.
- **What a page takes** is a list, by extension: images (PNG, JPEG, GIF, WebP), documents, text and data, spreadsheets,
  slides, and archives. Anything else is refused, which is where HTML, scripts, and programs go. SVG is taken, as
  diagrams come in it, but only ever downloads, since it can carry a script. An image shows only when its first bytes prove
  it is that image. See `docs/security.md`, Page attachments.
- **Who attaches.** Everyone who edits a page: people, and agents acting for them or as themselves, through functions
  taking an `Author`. A file shows on its page and is what its text links to, so unlike a label it is what a page
  presents, and a proposal-only page refuses agents here too (`page_proposal_only`). A fixed page takes files, since it
  takes edits.
- **Removing one** is soft. Its row keeps `deleted_at`, its address answers `404`, and an image in the text says it is
  unavailable. Its file stays with the provider, and the entry in the page's history restores it, at the same address.
- **A deleted page keeps its files.** It reads on at its address, so they are listed and served, and changes no further:
  adding, removing, or restoring one answers `409 page_deleted`. Restoring the page finds them as they were.
- **Nothing is deleted for good yet.** Pages and attachments are only ever deleted softly, so today every file stays with
  its provider for as long as the location does, whether its page or its attachment was deleted. Deleting a page for
  good is on the roadmap, and is where files finally go: it reads `storage_location_id` and `storage_path` from a page's
  rows, removed ones included, deletes each file, then the rows. Neither of that table's foreign keys cascades, so nothing
  can drop a row, and with it the only record of a file, by accident.

### Projects

With one tree and no spaces, a project shows its docs through links: a page may be linked to any number of projects, and
a project to any number of pages (`crate::models::page_project`, `page_projects`).

- **A link is not a version.** It changes nothing a page says, so it is an entry in the page's history alone (History,
  above) and leaves the index as it is. `page.changed` announces it.
- **It changes nobody's access.** Every member reads every page and no guest reads any, linked or not. A guest on a
  project sees no docs there: `GET /api/v1/projects/{id}/pages` refuses them with `403 members_only`, as every page route
  does, and the web app shows them no Docs tab.
- **People and agents link alike.** People link from a page or a project's Docs tab, and agents with
  `page_project_link` and `page_project_unlink`, over MCP and in every turn but a pool worker's, which only reads. Each
  write takes an `Author`, so an agent is named in the page's history as it is for a publish. Any page can be linked, a
  proposal-only or fixed one included: those rules guard what a page says and where it sits, and a link touches neither. Linking a page already linked, or taking off a link that is not there, records
  nothing.
- **A deleted page keeps its links** and takes no new ones, so it comes back as it went. A project lists live pages
  alone.
- **Deleting a project takes its pages off it** in the same transaction, each recorded in the page's history as a
  `project_unlinked`, deleted pages included. The pages stay.
- **Searching by project finds them.** The search's `project` filter keeps a linked page, learning, or company doc section
  beside what is attributed to the project, and it only narrows, as every filter does. `knowledge_get` names a page's
  projects, as it does a timeline record's.
- **Project Managers read them.** Every turn's prompt names its project's docs, and `manager_pages` and `manager_page`
  list and read them (`docs/engine.md`, Project Managers). Linking a page wakes nobody.
- **Every project has a brief.** Creating a project creates a page under Project briefs, titled for it ("Launch
  brief"), holding three sections (Goals, Decisions, Out of scope), linked to it, and named as its brief
  (`pages.brief_project_id`, `crate::models::project_brief`); renaming the project renames it. It is the system
  user's, since Elysium made it, and it is versioned, indexed, and edited like any page, except that one still holding
  only that skeleton has no document, since it would answer every search for a goal. It is fixed, since its Project
  Manager reads it every turn and every batch's goal is checked against it: nobody moves, deletes, or renames it, and
  its link to its project is not taken off (`409 page_brief_linked`). Deleting the project leaves it an ordinary page,
  as every linked page stays. A publish of it by anyone but its own Project Manager or the system user wakes that
  Project Manager in the same transaction. See `docs/autonomy.md`, The project brief.

### In the index

A page is a source of the index (Search, above): one document, reading as its newest version, upserted in the
transaction that publishes it, so a search finds what it says the moment it commits. Its text is its title and body,
then its evidence on a page that has any, cut into passages as every source is. It is titled as the page is, under the
source `page`, dated when its newest version was published, and attributed to who published it, since those are its
words now. It has no `url`: a page lives in Elysium, so a citation opens it by id, and `knowledge_get` answers its
origin as `{ kind: "page", pageId }`. A deleted page leaves the index and comes back with its page.

A learning's document keeps the source `learning`, and a section's `company_doc` with the title "Company doc: Glossary"
and so on, so the search's filter and citations name them as they always have; `knowledge_get` answers their origins
as `{ kind: "learning", learningId }`, the page's id, and `{ kind: "companyDoc", section, pageId }`. A fixed page is
indexed from its body alone, so one that says nothing has no document.

### Finding pages

Three ways, each for what it is best at, and none of them a second index.

- **By title and text, on the Knowledge area.** The search's Pages tab is the knowledge search under the sources `page`,
  `learning`, and `company_doc`: the same function, the same ranking, and the same budget as searching everything. It
  needs no scope of its own in the API, so anything that asks for those sources, an agent included, finds what a
  person finds.
- **By title as it is typed, in the tree.** The panel's field narrows the tree in the browser, from what the tree already
  loaded (Filtering the tree, `docs/frontend.md`), by the rule the global search reads a title with: every word typed
  begins a word of the title.
- **From anywhere, in the topbar.** A page's title is one more kind of record in `GET /api/v1/search`, matched as it is
  typed through `pages_title_search`, since full text matches whole words only (`runb` never finds Runbook) and a
  search by meaning would embed a query per keystroke. A guest's search holds none. The knowledge search answers
  beside it with what pages say, and a page both find is one result, which opens at the passage.

**A learning or a company doc section is a page here.** They sit in the tree and open as pages, and someone looking
for how the team restarts a fleet does not care which kind of page wrote it down, so the Pages tab finds all three, and its
filters can leave out a kind. Their own lists stay where they are.

**Author and date are the newest version's.** Pages are filtered by who last published and when, labelled in the
words of the page's byline (Last edited by, Edited since, Edited until). That is what each result shows and what its
document holds (In the index, above): those are its words now. Anyone who ever published a version was the alternative,
which would find a page for someone whose words may no longer be in it, under a name the result does not show.
Machines count: Elysia and agents publish as themselves, so the filter offers them where the rest of the search offers
people alone.

**A result opens the page, at the passage.** Its citation names the page (`pageId`), so it opens
`/knowledge/pages/<id>?passage=<position>`, as an answer's citation does, rather than the document view, which stays for
timeline records. A passage is a cut of Markdown, so the page reads it back from the index (`GET
/api/v1/pages/{id}/passages/{position}`) and finds it in the page's Markdown, rather than cutting the text a second time
in the browser and having to agree with `passages::split` forever. The place found is an offset into the Markdown, and
every rendered block still knows its offsets, so marking one needs no matching of rendered text, which has lost a
table's pipes and a link's address. The page opens with the passage marked and in view. It opens at its top, with
nothing marked, for whatever the page does not draw there: a link older than the text, evidence, or a footnote. Publishing
keeps the mark where the text still is and never moves a reader. The History tab and the editor take no part, and coming
back to the page's own tab opens at the passage again.

### Learnings and the company doc

A learning is a page with a category and evidence, created under the Learnings page, and keeps its id as the page's. The
company doc is seven proposal-only pages under the Company page, each section's id fixed in code
(`Section::page_id`). Both are written through what every publish comes down to (`page::publish_content`), so each
change is a version with who made it and the agent acting for them, in the index in the same transaction, and they add
only their own rules: saving by title and the category's checks (`crate::models::learning`), and the fixed sections and
the review (`crate::models::company_doc`). Suggestions are every page's (Suggestions, above).

- **A learning is what it holds, not where it sits.** `pages.category` makes a page a learning, wherever it is moved;
  a page placed under Learnings without one is an ordinary page and is not listed as a learning. No page becomes a
  learning by publishing one: `409 page_not_a_learning`.
- **Their bounds stay.** A learning's body is 1 to 20,000 characters and a section's at most 12,000, in the database and
  whichever way it is written, so the review's one prompt still fits the doc.
- **Undo.** Undoing is generic over a page's fields (`page::undo_version`): each field that still holds what a version
  wrote gets back what it replaced, a field changed again since keeps its newer value, and the reversal is the next
  version, naming the entry it undid. A learning's undo works on its four fields. Undoing a learning's first save or
  restore deletes it, and undoing a deletion restores it, through the page's own delete and restore. A company doc
  section is restored as any page is.
- **Every entry reads as it did.** The routes for learnings answer the same shapes they always have, from the pages'
  versions and history: an entry written before learnings were pages keeps its id and can be undone. Its `revised` kind
  is what a `published` row reads as there.
- **What people see.** The Learnings tab and the admins' feed work as they did, each entry with its Undo, and every
  revision and section change is also a version of its page, which can be read, compared, and restored like any other.
  A learning's address (`/knowledge/learnings/<id>`) and a citation open its page, and Edit opens the editor, with its
  evidence and category, while History and undo goes to `/knowledge/learnings/<id>/edit`. A citation of the company doc
  opens its section's page, where Edit opens the editor as a person acting alone and Suggestions reviews what agents
  suggested. A move shows in a page's history and not in a learning's. Restoring a version puts back the whole of it,
  where Undo puts back only what is still there to reverse. A learning's search result is attributed to who published
  its newest version, rather than who first saved it, as a page's is.
- **Moved in one migration, and back.** Revisions replay into versions from each one's own history, and a history that
  does not replay onto its row stops the migration, which changes nothing; a document moves to its page's column in
  place, so its passages keep their embeddings, and its text is what they were embedded from, so indexing it again
  re-embeds nothing. `down.sql` hands every row back exactly, and carries what was written since in the old tables'
  shapes; pages of someone's own left under what goes move to the top of the tree.

### In the web app

The Knowledge area (`/knowledge`) opens on its first page, beside the tree. A panel on the left holds Search, Ask, and
Learnings, then every page, so everything in the area sits beside the tree; below `lg` it is a drawer. Each page has its
own address (`/knowledge/pages/<id>`), and reads as its path through the tree, its title, who last edited it and when,
its labels, the projects it is linked to, its text, its attachments, and its child pages. A label opens every page
carrying it (`/knowledge/labels/<label>`), and the panel's filters narrow the tree by title as it is typed and by label,
to what matches and the pages above it, opened. No title matching offers a search of what pages say (Finding pages,
above). Which branches a person leaves open stays with them on each browser. A deleted page reads
on at its address, marked deleted and off the tree. See `docs/frontend.md`, Pages.

The tree is also where people organize pages. Each row's menu adds a child page, moves the page, or deletes it. A page
drags by pointer or by keyboard to another place among its siblings or under another page, with everything under it,
and a target under the page itself refuses it. Where dragging cannot reach, such as on a touch screen, Move asks where
it goes and after which sibling. A move can be undone from its toast, and shows in the page's history. Deleting happens
at once, with Undo in its toast, except that a page with pages under it first asks what happens to them: left as they
are they move up into its place, and a switch deletes them too. Whoever was reading what went is taken to its parent,
and Undo brings them back; once its toast has gone, a deleted page's own address offers Restore. The fixed pages,
the ten Elysium seeds and every project's brief, take children, but nobody can drag, move, or delete one; a lock on the
row and the row's menu say why. A brief names its project with no X to take it off, and the project's Docs tab offers
no Unlink for it.

A page's text is Markdown, read to React elements, with GitHub's tables, task lists, and footnotes, and code highlighted
for a block that names a common language. Agents write pages, so nothing in one is trusted: raw HTML shows as text and
runs nothing, and only `http`, `https`, `mailto`, and relative addresses survive. An image on another origin never
loads, since the Content Security Policy refuses it and loading it would tell a third party who read the page: it is a
link that opens the image in a new tab. A file attached to a page is served by Elysium, so an image shows inline, and a
link to any other file downloads it.

A page's Suggestions tab, between Read and History and counting what waits, reviews the edits agents suggested to it,
oldest first: who suggested it and through which agent, when, the review's run it came from, its reason, its quote and
source, and each of its changes as a diff of the lines around it, with a checkbox to take it when there are several. A
change that no longer merges with the page says so, and a suggestion written against an earlier version says that
accepting merges both. Accept takes the changes ticked, all of them at first, and acts at once with a toast; Dismiss
asks for the reason in a dialog. A page's row in the tree, its result in the topbar's search, and its card in the
knowledge search show how many suggestions wait.

A page's History tab lists everything that happened to it, newest first: each version with who published it, the agent
acting for them, and when, and between them its moves, deletions, restorations, labels, and links to projects, so what learnings and the
company doc carried over reads here too. A version opens on what it changed from the one before. Ticking any two compares them, older
against newer, line by line on the Markdown with the changed words marked, and a change to a title, category, or
evidence shows the same way. Read shows a version as it read, in the page's own layout. Restore acts at once, with Undo
in its toast, since nothing is lost: Undo restores the version it replaced, and both stay versions. A person restores a
proposal-only page as they edit one. Restore is not offered on a deleted page, which refuses it, and waits while the
page has unsaved edits.

A page shows the projects it is linked to, each opening that project's docs, with a field that adds another and an X that
takes one off. A project's Docs tab lists the pages linked to it, with their count on the tab, and links another picked
from the tree by title or by where it sits. Each is an action, not a field of either, so it applies at once, with Undo
in its toast, and asks for no Save or Publish. A guest has no Docs tab.

People edit a page in a rich editor at `/knowledge/pages/<id>/edit`, and create one at `/knowledge/pages/new`. The page
stays Markdown, so agents read and write it plainly and diffs stay readable: every block nobody touched keeps its
exact text, so a page opened and published as it was is no version and no diff, and an edit's diff is the edit. Publish
is the page's one Save. Someone who publishes over a version they have not seen is told, and shown the difference, and
keeps their text. What the editor has no form for opens as its Markdown, and is never lost. See `docs/frontend.md`, The
Markdown editor.

A page's Attachments list each file with its size, who added it, and when, images as small pictures that open larger.
Files attach by the button there or dropped on it, and in the editor by its toolbar's button, dropped on the text, or
pasted, as a screenshot is, in which case each holds its place in the text until it arrives. A page being created has
nothing to attach to yet, and says so. Removing a file happens at once, with Undo in its toast, and its entry in the
page's history restores it afterwards.

### Roadmap

- **Meetings.** Meetings become pages of their own kind, public by default with private ones found only by their
  readers, under three new fixed pages: Meetings, Projects, and Transcription. The design is `docs/meeting-review.md`.
- **Deleting for good.** A page, with its versions and its attachments' files, is removed for good (Attachments, above).
- **Attachments for agents.** The page tools read a page's references to its files, but not the files, whose address
  needs a person's session, and attach nothing. Tools to fetch and attach one would call what the routes call
  (`upload_attachment::upload_file`, `remove`, `restore`), each taking an `Author`.

## Onyx

Mooreslab's Onyx knowledge base and its knowledge-save sidecar keep running beside Elysium for now, with agents
writing to both. Retiring it, and whether to import its saved learnings, is decided later. The sidecar files learnings
under a fixed list of its own; an import maps that list onto the built-in categories and the workspace's own.
