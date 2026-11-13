# Database

Postgres 18, pgvector's build of it, accessed from the API through Diesel 2.3 with `diesel-async` and a `deadpool`
connection pool.

## Why Diesel

Queries are checked at compile time against `api/src/database/schema.rs`. A wrong column name, a type mismatch,
or a nullable column read as non-null is a build error, not a runtime surprise. `schema.rs` is generated from the
migrations, so the compiler checks the code against the real schema.

Diesel is built with its `64-column-tables` feature: `work_batches` holds more than 32 columns, Diesel's default
limit for one table.

## Migrations

Each migration is a directory under `api/migrations/` holding `up.sql` and `down.sql`. Create one with the pinned
Diesel CLI:

```sh
cargo install diesel_cli --version 2.3.13 --no-default-features --features postgres --locked
cd api && diesel migration generate <name>
```

Migrations are compiled into the API binary with `embed_migrations!`. The binary manages them itself:

```sh
elysium-api migrate run              # apply everything pending
elysium-api migrate status           # list applied and pending
elysium-api migrate revert           # roll back the latest migration
elysium-api migrate revert --count 3 # roll back the latest three
elysium-api migrate revert --all     # roll back everything (destroys data)
elysium-api migrate redo             # roll back the latest, then re-apply it
```

In compose, run these as `docker compose run --rm migrate elysium-api migrate <command>`.

Guarantees:

- Each migration runs in its own transaction. A failing migration leaves no partial schema.
- Every migration command takes a Postgres advisory lock first. Concurrent migrators wait instead of racing.
- The API checks for pending migrations before binding its port. It refuses to serve against an older schema.
- In compose, the one-shot `migrate` service runs `migrate run`. The API starts only after it exits successfully.
- `migrate run` first rebuilds the indexes of any database whose collation changed (see Collations below).
- Migrations run as `POSTGRES_USER`, the cluster's bootstrap superuser, in every stack and in verification. Creating
  an extension that is not marked trusted, such as `vector`, needs one.

## Collations

Text indexes are kept in the order the database's collation gave when each key went in. The collation comes from the
server's C library (`en_US.utf8`, libc provider), so an image on another C library, or another release of glibc,
sorts the same keys differently: lookups then miss rows that are there, and unique indexes stop refusing duplicates.
Postgres records the collation's version (`pg_database.datcollversion`) and only warns when it changes.

`migrate run` checks Elysium's database, Kratos's, and Hydra's before it applies anything. Where the recorded version
differs from the one the library reports now (`pg_database_collation_actual_version`), it runs `REINDEX DATABASE`
connected to that database, records the new version in `pg_database` itself, and logs `migration.collation.rebuilt`
at warn level. It writes the catalog rather than running `ALTER DATABASE ... REFRESH COLLATION VERSION`, which refuses
a change from or to no version, as moving between musl and glibc is; that takes the superuser migrations already run
as. A database whose collation held is left alone. A rebuild that meets
duplicates, written while the old indexes were in the wrong order, fails the migration and says which index. The code
is `rebuild_for_changed_collation` in `api/src/database/migrations.rs`; `REINDEX DATABASE` cannot run in a
transaction, so it is not a migration.

A cluster first started under the Alpine image recorded no version, since musl reports none, and its indexes are in
musl's byte order; under pgvector's Debian image the first `migrate run` rebuilds every one. A glibc update in a later
pgvector image is caught the same way. See `docs/infrastructure.md`, Upgrading Postgres's image.

## Verification

`api/scripts/verify-migrations.sh` starts a disposable Postgres container, the image `compose.yml` runs, and runs these
checks:

1. `diesel migration run --locked-schema` fails if `schema.rs` differs from what the migrations produce.
2. `diesel migration redo --all --locked-schema` proves every `down.sql` undoes its `up.sql`.
3. `cargo test -- --include-ignored` runs the store-backed tests, against that Postgres and a disposable Redis.

The database-backed tests give each test its own database. They cover up, down, and up again, single reverts,
redo, concurrent migrators, the pending-migration refusal, the rebuild for a changed collation, the knowledge
index's cascades and columns, timeline records indexed as they are stored with a page's newest version alone, the
backfill's walk across batches and its resume, passages embedded by the embedder against a fake embedding service,
the knowledge search's two rankings, filters, folding, documents, refusal of guests, and budget through the router,
learnings saved, revised, undone, deleted, and indexed with them, pages' labels and the search's filter by them, edits
suggested to pages, accepted wholly and in part, merged, refused, and dismissed, with each page's one inbox item, the move
of learnings and the company doc onto pages against data laid out as production's, up, down to the row, and up again,
with no passage re-embedded, the company doc's proposals becoming suggestions the same way, the split of
meetings into captures the same way, each final transcript built word for word as the code builds one, meetings'
edits and marks moved and undone over their words, learning categories
added, reordered, and removed with their learnings moved, and every LLM,
storage location, GitHub
credential, Jira credential, environment variable, coding session, action item, initiative, comment, history, and link
query, every `elysium_work` tool against its project scope, each link provider against a faked provider, the
watcher's passes, changesets proposed, decided, applied in part, and undone, workflow runs claimed by two claimers at
once, leased, taken back, backed off, retried, skipped, and resumed, workflows composed from the same steps, entered
at a later step, and read on their steps' model settings, accounts and their guardrails, the global search's matching, visibility, and budget, and the
router itself refusing every workspace route without a session. Plain `cargo test` skips them because they need `TEST_DATABASE_URL`, and the router tests
`TEST_REDIS_URL`. A router test signs in by caching a Kratos session in Redis (`test_support::signed_in_cookie`).

## Conventions

- Timestamps are `TIMESTAMPTZ` and map to `chrono::DateTime<Utc>`. The server runs with `timezone=UTC`.
- Primary keys are UUIDv7, generated by the API. `coding_sessions` is the exception: its ids are numbers people
  read, from an identity column.
- Tables with `updated_at` call `diesel_manage_updated_at('<table>')` so a trigger maintains it.
- Enums are native Postgres enums mapped with `diesel-derive-enum`, stored in `snake_case`.
- Constraints such as length checks and uniqueness are enforced in SQL as well as in request validation.
- Each model module under `api/src/models/` owns its queries. Handlers call those functions and never build
  queries themselves.
- Every top-level table has `created_by UUID NOT NULL REFERENCES users`. Join and child tables inherit their parent's.
  See `docs/auth.md`.

## Searching titles

The global search (`GET /api/v1/search`, `docs/api.md`) finds action items, projects, coding sessions, initiatives,
and pages by their titles through `pg_trgm`, created by the migration that adds its first indexes:

- `action_items_title_search`, `initiatives_name_search`, and `pages_title_search`, where `deleted_at` is NULL;
- `projects_name_search`, over every project;
- `coding_sessions_title_search`, where `studio_item_id` is NULL.

Each is a GIN index with `gin_trgm_ops`, partial on exactly the rows its list shows, so the search's query, which
repeats that condition, can use it. Every typed word is matched as `title ~* '\mword'`, which a trigram index
answers, and results rank by `similarity`. Full text was the other candidate, but its parser keeps `WAY-123` as `way`
and `-123`, and a host or a path such as `src/main.rs` as one token, so the keys, hosts, and paths these short titles
hold would not be found by their parts. A word under three characters has too few trigrams for the index, and is
matched by scanning instead, which tables of this size answer at once.

## Tables

### `llms`

| Column                   | Type          | Notes                                                                         |
|--------------------------|---------------|-------------------------------------------------------------------------------|
| `id`                     | `UUID`        | UUIDv7, primary key                                                           |
| `name`                   | `TEXT`        | 1 to 120 characters, unique                                                   |
| `description`            | `TEXT`        | Up to 2000 characters, defaults to empty                                      |
| `type`                   | `llm_type`    | `chatgpt_oauth`, `chatgpt_api_token`, `claude_api_token`, `claude_code_oauth`, `claude_account` |
| `secret_token_encrypted` | `BYTEA`       | Sealed envelope, see `docs/secrets.md`; a `claude_account`'s current access token |
| `priority`               | `INTEGER`     | Lower is tried first, defaults to 0                                           |
| `is_active`              | `BOOLEAN`     | Defaults to true                                                              |
| `expires_at`             | `TIMESTAMPTZ` | Optional; null means no expiry                                                |
| `created_at`             | `TIMESTAMPTZ` | Set on insert                                                                 |
| `updated_at`             | `TIMESTAMPTZ` | Maintained by trigger                                                         |

The partial index `llms_active_priority_idx` on `(priority, created_at) WHERE is_active` serves the lookup for
active credentials in priority order.

### `claude_accounts`

What keeps a `claude_account` credential signed in, one row per credential, deleted with it. See `docs/coding.md`,
Claude accounts. It extends one `llms` row rather than standing on its own, so it has no `created_by` of its own.

| Column                     | Type               | Notes                                                              |
|----------------------------|--------------------|--------------------------------------------------------------------|
| `llm_id`                   | `UUID`             | Primary key, references `llms` on delete cascade                   |
| `account_uuid`             | `UUID`             | Anthropic's account id, unique: signing it in again replaces tokens |
| `email`                    | `TEXT`             | 1 to 320 characters                                                |
| `organization_uuid`, `organization_name` | `UUID`, `TEXT` | Optional, as the profile reports them                     |
| `subscription_type`        | `TEXT`             | Optional, such as `max`                                            |
| `refresh_token_encrypted`  | `BYTEA`            | Sealed envelope, see `docs/secrets.md`                             |
| `access_token_expires_at`  | `TIMESTAMPTZ`      | When the access token on the credential lapses                     |
| `refresh_token_expires_at` | `TIMESTAMPTZ`      | Optional; null when Anthropic did not say                          |
| `sign_in_state`            | `TEXT`             | `signed_in` or `expired`; only a new sign-in clears `expired`      |
| `last_refreshed_at`, `last_refresh_error` | `TIMESTAMPTZ`, `TEXT` | The last refresh, and why the last one failed            |
| `five_hour_utilization`, `seven_day_utilization` | `DOUBLE PRECISION` | Latest usage, as a percentage from 0              |
| `five_hour_resets_at`, `seven_day_resets_at` | `TIMESTAMPTZ` | When each window resets                                     |
| `usage_read_at`, `usage_error` | `TIMESTAMPTZ`, `TEXT` | The last usage read, and why the last one failed               |
| `created_at`, `updated_at` | `TIMESTAMPTZ`      | `updated_at` maintained by trigger                                 |

### `mail_accounts`

Connected mailboxes; see `docs/mail.md`.

| Column                 | Type                | Notes                                                           |
|------------------------|---------------------|-----------------------------------------------------------------|
| `id`                   | `UUID`              | UUIDv7, primary key                                             |
| `kind`                 | `mail_account_kind` | `gmail`, `outlook`, `self_hosted`                               |
| `address`              | `TEXT`              | Lowercase, unique, up to 320 characters                         |
| `display_name`         | `TEXT`              | Sender name, up to 120 characters, defaults to empty            |
| `credential_encrypted` | `BYTEA`             | Sealed refresh token or password, see `docs/secrets.md`         |
| `external_id`          | `TEXT`              | Stalwart's account id; set exactly when `kind` is `self_hosted` |
| `mail_domain_id`       | `UUID`              | `mail_domains`; set exactly when `kind` is `self_hosted`, `RESTRICT` on delete |
| `is_active`            | `BOOLEAN`           | Defaults to true                                                |
| `last_checked_at`      | `TIMESTAMPTZ`       | Latest connection check; null until one runs                    |
| `last_error`           | `TEXT`              | Why that check failed; null when it passed                      |
| `created_at`           | `TIMESTAMPTZ`       | Set on insert                                                   |
| `updated_at`           | `TIMESTAMPTZ`       | Maintained by trigger                                           |

### `password_vault`

The one Bitwarden or Vaultwarden account agents sign in to things with; see `docs/passwords.md`. One row at most.

| Column                      | Type              | Notes                                                       |
|-----------------------------|-------------------|-------------------------------------------------------------|
| `singleton`                 | `BOOLEAN`         | Primary key, always true                                    |
| `server`                    | `password_server` | `bitwarden_us`, `bitwarden_eu`, or `self_hosted`            |
| `server_url`                | `TEXT`            | A self-hosted server's https URL, set exactly when `server` is `self_hosted` |
| `client_id`                 | `TEXT`            | The personal API key's client_id, 1 to 200 characters       |
| `client_secret_encrypted`   | `BYTEA`           | Sealed, see `docs/secrets.md`                               |
| `master_password_encrypted` | `BYTEA`           | Sealed, see `docs/secrets.md`                               |
| `account_email`             | `TEXT`            | The account the key signs in as, as the CLI reported it     |
| `collection_id`, `collection_name`, `organization_id` | `UUID`, `TEXT`, `UUID` | The one collection agents read; all three set together, or none |
| `checked_at`                | `TIMESTAMPTZ`     | When the CLI last signed in and unlocked with the credentials |
| `created_by`                | `UUID`            | References `users`; kept when the credentials are replaced  |
| `created_at`, `updated_at`  | `TIMESTAMPTZ`     | Set on insert; `updated_at` maintained by trigger           |

### `browser_sessions`

Browsers the host runs as connectors' accounts, one Neko container each, which a person signs in to on its
connector's page; see `docs/browser.md`. Each belongs to the person who created it and answers nobody else, and to the
one connector that reads through it.

| Column                      | Type          | Notes                                                           |
|-----------------------------|---------------|-----------------------------------------------------------------|
| `id`                        | `UUID`        | UUIDv7, primary key; names its container, volume, and network   |
| `name`                      | `TEXT`        | 1 to 100 characters: its connector's kind, such as `outlook`    |
| `start_url`                 | `TEXT`        | `https://`, at most 2048 characters: its kind's start page, opened when it is created |
| `start_page_opened_at`      | `TIMESTAMPTZ` | When `start_url` was opened in it; null until Chromium first answers |
| `port`                      | `INTEGER`     | Unique, 59000 to 59007: its media port, and its name on its network |
| `member_password_encrypted` | `BYTEA`       | Sealed password of the Neko member session the API relays, see `docs/secrets.md` |
| `state`                     | `browser_state` | `starting`, `running`, or `failed`, as the engine's reconciler last found it |
| `state_error`               | `TEXT`        | Why it failed; set exactly while `failed`                       |
| `refreshed_at`              | `TIMESTAMPTZ` | When the engine last reloaded its service so the session renews, recorded as it starts; null until the first, see `docs/browser.md`, Refreshing |
| `created_by`                | `UUID`        | References `users`; the browser's owner, indexed                |
| `created_at`, `updated_at`  | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger               |

### `token_accounts`

A person's accounts whose whole sign-in is one API key, such as Fireflies and YouTrack; see `docs/connectors.md`.

| Column                      | Type          | Notes                                                           |
|-----------------------------|---------------|-----------------------------------------------------------------|
| `id`                        | `UUID`        | UUIDv7, primary key                                             |
| `kind`                      | `TEXT`        | The provider, such as `fireflies` or `youtrack`                 |
| `site_url`                  | `TEXT`        | The instance the key was issued on, an https origin; required for `youtrack`, absent for a provider with one fixed host |
| `name`                      | `TEXT`        | 1 to 120 characters                                             |
| `credential_encrypted`      | `BYTEA`       | The sealed key, bound to the row's id, see `docs/secrets.md`    |
| `identity`                  | `TEXT`        | Who the provider said the key belongs to; at most 512 characters |
| `checked_at`                | `TIMESTAMPTZ` | When the provider last confirmed the key                        |
| `created_by`                | `UUID`        | References `users`; the account's owner                         |
| `created_at`, `updated_at`  | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger               |

### `connectors`

A service read as its person, through an account the connector owns; see `docs/connectors.md` and
`docs/meetings.md`. Exactly one of the four account columns is set (`connectors_one_sign_in`), and each is unique on its
own, so no two connectors share an account. The one exception is an Outlook, Gmail, Teams, or Slack connector that
shared its browser before every connector owned its own: it has none while it waits for a browser of its own.

| Column                      | Type          | Notes                                                           |
|-----------------------------|---------------|-----------------------------------------------------------------|
| `id`                        | `UUID`        | UUIDv7, primary key                                             |
| `kind`                      | `TEXT`        | The kind, such as `jira`, `outlook`, or `fireflies`             |
| `browser_session_id`        | `UUID`        | References `browser_sessions`, cascading: a mail or chat connector's browser |
| `token_account_id`          | `UUID`        | References `token_accounts`, cascading: Fireflies' or YouTrack's key |
| `jira_credential_id`        | `UUID`        | References `jira_credentials`, cascading: Jira's or Confluence's Atlassian account |
| `github_credential_id`      | `UUID`        | References `github_credentials`, cascading: GitHub's account    |
| `content`                   | `JSONB`       | What it reads, in its kind's terms, for a kind without a table of its own, such as GitHub's `{"repositories": "*"}` |
| `storage_location_id`       | `UUID`        | References `storage_locations`: where it keeps files, such as meeting recordings |
| `sign_in_state`             | `TEXT`        | `signed_in`, `signed_out`, or `unknown`, as last read           |
| `cursor`                    | `JSONB`       | Where the last read left off, in the kind's terms               |
| `last_polled_at`            | `TIMESTAMPTZ` | When it was last read                                           |
| `last_error`                | `TEXT`        | Why the last read failed, or `NULL`                             |
| `is_active`                 | `BOOLEAN`     | False while paused                                              |
| `syncing_since`             | `TIMESTAMPTZ` | Set while a read someone asked for waits or runs, or a meeting's processing runs; the engine clears it |
| `created_by`                | `UUID`        | References `users`; its creator                                 |
| `created_at`, `updated_at`  | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger               |

### `secret_copies`

Secrets a migration copied into new rows, still sealed for the rows they came from, since SQL cannot seal. The API
re-seals each for its own row when it starts, before anything reads it, and removes it from here; see
`docs/secrets.md`. Empty once the API has started. Primary key `(target, row_id)`.

| Column       | Type   | Notes                                                                     |
|--------------|--------|---------------------------------------------------------------------------|
| `target`     | `TEXT` | Where the copy lives, as `<table>.<secret>`: today `jira_credentials.token` |
| `row_id`     | `UUID` | The row holding the copy                                                  |
| `sealed_for` | `UUID` | The row the copied envelope is still bound to                             |

### `connector_check_results`

The last result of each of a connector's checks. Primary key `(connector_id, check_id)`; cascades with the connector.

| Column         | Type          | Notes                                                           |
|----------------|---------------|-----------------------------------------------------------------|
| `connector_id` | `UUID`        | References `connectors`                                         |
| `check_id`     | `TEXT`        | The check, as its kind names it, such as `sign_in`              |
| `state`        | `TEXT`        | `passed`, `failed`, or `skipped`                                |
| `count`        | `BIGINT`      | How many it found, for a check that counts                      |
| `reason`       | `TEXT`        | Why, in Elysium's own terms, such as `no_messages`              |
| `message`      | `TEXT`        | What the provider said; at most 2048 characters                 |
| `blocked_by`   | `TEXT`        | The check it waited on, when skipped because that one did not pass |
| `ran_at`       | `TIMESTAMPTZ` | When it ran                                                     |

### `connector_runs`

Syncs worth showing: those that recorded something, and those that failed. Each connector keeps its newest 50;
indexed by connector and by start time.

| Column         | Type          | Notes                                                           |
|----------------|---------------|-----------------------------------------------------------------|
| `id`           | `UUID`        | UUIDv7, primary key                                             |
| `connector_id` | `UUID`        | References `connectors`, cascading                              |
| `started_at`, `finished_at` | `TIMESTAMPTZ` | When it ran                                        |
| `outcome`      | `TEXT`        | `succeeded` or `failed`                                         |
| `records`      | `INTEGER`     | How many things it recorded: records, meetings, or linked changes |
| `error`        | `TEXT`        | Why it failed; at most 2048 characters                          |

### `conversations`

Where direct messages and mentions gather per conversation before a request run starts work on them, and where the
agent handling a conversation keeps its session (`docs/engine.md`, Conversations). Unique per connector and key;
indexed by owner.

| Column               | Type          | Notes                                                                    |
|----------------------|---------------|--------------------------------------------------------------------------|
| `id`                 | `UUID`        | UUIDv7, primary key; a request run's source id                           |
| `connector_id`       | `UUID`        | References `connectors`, cascading                                       |
| `key`                | `TEXT`        | The conversation in its kind's own terms; 1 to 1024 characters           |
| `title`              | `TEXT`        | What it is called where it is had, as its newest waking message named it; 1 to 2048 characters |
| `harness_session_id` | `TEXT`        | The Claude session its agent's turns resume; null until its first turn   |
| `waiting_until`      | `TIMESTAMPTZ` | When a wait its agent asked for ends, after which it is owed a turn; null while none is waited out |
| `waited_in_run_id`   | `UUID`        | References `workflow_runs`, set null: the run whose agent used its one wait |
| `created_by`         | `UUID`        | References `users`: the connector's person, whose conversation it is     |
| `created_at`, `updated_at` | `TIMESTAMPTZ` |                                                                |

### `conversation_messages`

Each message a conversation gathered, and how far work on it got. A child of its conversation, whose owner it takes.
Indexed by conversation and arrival, and partially over what still waits on a run or an agent.

| Column               | Type          | Notes                                                                    |
|----------------------|---------------|--------------------------------------------------------------------------|
| `timeline_record_id` | `UUID`        | Primary key; references `timeline_records`, cascading                    |
| `conversation_id`    | `UUID`        | References `conversations`, cascading                                    |
| `wakes`              | `BOOLEAN`     | Whether it asks something of the person, which is what starts work       |
| `received_at`        | `TIMESTAMPTZ` | When Elysium read it; the quiet window runs from the newest              |
| `run_id`             | `UUID`        | References `workflow_runs`, set null: the request run that took it, whose segmenting reads it |
| `delivered_at`       | `TIMESTAMPTZ` | When a handling agent was given it                                       |

### `timeline_records`

Everything ingested, newest first by `occurred_at` (indexed). Unique on `(connector_id, external_id)`, so an
item is recorded once per connector, and on `(turn_summary_session_id, turn_summary_turn_id)`, so a turn summary is
one record. `timeline_records_by_ingestion` on `(created_at, id)` reads it in the order it was ingested, as the company
doc's review does. A meeting's excerpts, and so the projects a meeting's excerpts are on, which listing meetings reads,
are found through their marks (`excerpt_mark_id`, unique).

| Column                 | Type          | Notes                                                                   |
|------------------------|---------------|-------------------------------------------------------------------------|
| `id`                   | `UUID`        | UUIDv7, primary key                                                     |
| `source_kind`          | `TEXT`        | What it is, such as `outlook` or `meeting`                              |
| `connector_id`         | `UUID`        | References `connectors`; set to `NULL` when the connector goes          |
| `external_id`          | `TEXT`        | The provider's id, at most 1024 characters                              |
| `occurred_at`          | `TIMESTAMPTZ` | When it happened, or when it was read                                   |
| `actor`                | `TEXT`        | Who it came from, as the provider shows them                            |
| `title`, `body`        | `TEXT`        | Plain text                                                              |
| `url`                  | `TEXT`        | A link to it, when there is one                                         |
| `media`                | `JSONB`       | A span inside a recording, and its clip, for a meeting's excerpt        |
| `metadata`             | `JSONB`       | Whatever else the connector keeps                                       |
| `action_item_id`       | `UUID`        | References `action_items`: the item it raised; `NULL` when it is gone   |
| `created_by`           | `UUID`        | References `users`; whose timeline it is on, or for the workspace's own the Project Managers' machine user or the coding agent's |
| `created_at`           | `TIMESTAMPTZ` | Set on insert                                                           |
| `turn_summary_session_id`, `turn_summary_turn_id` | `BIGINT`, `UUID` | Together reference `turn_summaries`, cascading: the summary a `turn_summary` record is. Set on that kind alone, both or neither (`timeline_records_turn_summary_kind`) |
| `excerpt_mark_id`      | `UUID`        | Unique; references `meeting_marks`: the excerpt a meeting's record is (`docs/meetings.md`, Excerpts and black-outs); `NULL` for every other record, and for a meeting's record that names none of its words |

A Slack record's `metadata` keeps where its message was said (`team`, `conversation`, `ts`, and `threadTs` for a
reply), and a Teams record's its `conversationId`, `messageId`, `parentId` (a channel message's thread), and `isChat`,
so a reply can be sent to it. A record of a sent message also names the draft it was sent from (`draftId`). A mail sent
from a mailbox is recorded with `source_kind` `mail` and no connector.

### `drafts`

Messages a person sends as themselves, a draft until sent and kept after; see `docs/connectors.md`, Sending. Indexed by
record and by person.

| Column               | Type          | Notes                                                                   |
|----------------------|---------------|-------------------------------------------------------------------------|
| `id`                 | `UUID`        | UUIDv7, primary key                                                     |
| `reply_to_record_id` | `UUID`        | References `timeline_records`, `SET NULL`: the record it answers        |
| `connector_id`       | `UUID`        | References `connectors`, cascading: the connector that sends a chat reply |
| `mail_account_id`    | `UUID`        | References `mail_accounts`, cascading: the mailbox mail goes from; exactly one sender is set (`drafts_one_sender`) |
| `target`             | `JSONB`       | Where it goes, in its sender's terms: a Slack channel and thread, a Teams conversation and thread, or a mail's recipients and subject |
| `place`              | `reply_place` | `thread` or `conversation`, for a chat reply                            |
| `body`               | `TEXT`        | 1 to 20,000 characters                                                  |
| `state`              | `draft_state` | `draft`, `sending`, `sent`, or `failed`                                 |
| `last_error`         | `TEXT`        | Why the last send failed, set exactly while `failed`; up to 2000 characters |
| `attempted_at`       | `TIMESTAMPTZ` | When a send last started                                                |
| `sent_at`            | `TIMESTAMPTZ` | Set exactly while `sent`                                                |
| `sent_record_id`     | `UUID`        | References `timeline_records`, `SET NULL`: what the sent message became |
| `created_by`         | `UUID`        | References `users`; the person it is sent as                            |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger                 |
| `promise_id`         | `UUID`        | References `promises`, `SET NULL`: the promise a follow-up keeps; unique while set (`drafts_one_per_promise`). A follow-up never shows in its record's reply box |

### `timeline_record_projects`

Which projects a record is about. Primary key `(timeline_record_id, project_id)`, both cascading; indexed by project.

| Column               | Type          | Notes                                                       |
|----------------------|---------------|-------------------------------------------------------------|
| `timeline_record_id` | `UUID`        | References `timeline_records`                               |
| `project_id`         | `UUID`        | References `projects`                                       |
| `confidence`         | `REAL`        | 0 to 1; a person's attribution is 1                         |
| `attributed_by`      | `TEXT`        | The actor, as history writes it (`user:<id>`, `elysia`, `project_manager`, `session:<n>`) |
| `created_at`         | `TIMESTAMPTZ` | Set on insert                                               |

Every write goes through `models::attribution`, which records it in `timeline_record_events`, and a new attribution
by Elysia or a person wakes the project's Project Manager (`project_manager_wakes`) in the same transaction; what a
Project Manager or a session's agent attributes wakes nothing.

### `timeline_record_events`

The history of what records are attributed to, one row per change, shaped like `page_events`; see
`docs/connectors.md`, The timeline. Cascades with its record and its project. `timeline_record_events_record` on
`(timeline_record_id, id)` reads one record's history in order; `timeline_record_events_pair` on
`(timeline_record_id, project_id)` answers whether a person decided a pair, which Elysia then leaves alone.

| Column               | Type          | Notes                                                                 |
|----------------------|---------------|-----------------------------------------------------------------------|
| `id`                 | `UUID`        | UUIDv7, primary key; orders entries as they were written              |
| `timeline_record_id` | `UUID`        | References `timeline_records`, cascading                              |
| `project_id`         | `UUID`        | References `projects`, cascading                                      |
| `kind`               | `TEXT`        | `attributed` (added, or how sure it is changed) or `unattributed`     |
| `created_by`         | `UUID`        | References `users`: the record's owner, or Elysia's machine user      |
| `data`               | `JSONB`       | `{ before, after }`, each `{ confidence, attributedBy }` or `null`, and `undid` for an undo |
| `created_at`         | `TIMESTAMPTZ` | Written by the API, the moment of the change                          |

### `page_suggestions`

Edits agents suggested to a page, each waiting for a person to accept, wholly or in part, or dismiss with a reason
(`docs/knowledge.md`, Pages, Suggestions). The company doc's suggestions are those to its sections' pages. The partial
unique index `page_suggestions_pending_once` on `(page_id, md5(body))` while `pending` keeps the same edit pending once
on a page; `page_suggestions_pending` on `(page_id, id)` while `pending` reads what waits on a page and counts it for
the tree and search; `page_suggestions_by_state` on `(state, id)` reads the latest dismissed for the company doc's
review.

| Column               | Type          | Notes                                                                  |
|----------------------|---------------|------------------------------------------------------------------------|
| `id`                 | `UUID`        | UUIDv7, primary key; a suggestion to the company doc keeps the id it had as a proposal |
| `page_id`            | `UUID`        | References `pages`, cascading                                          |
| `base_version`       | `INTEGER`     | With `page_id`, references `page_versions`: the version it was written from |
| `base_body`          | `TEXT`        | That version's text, which its edits were applied to and which accepting merges from |
| `body`               | `TEXT`        | The page's text as it would leave it; never `base_body`; at most 100,000 characters |
| `reason`             | `TEXT`        | 1 to 2,000 characters                                                  |
| `quote`, `source`    | `TEXT`        | Where it was read; at most 2,000 and 500 characters                    |
| `timeline_record_id` | `UUID`        | References `timeline_records`, `SET NULL`: the record the company doc's review cited |
| `workflow_run_id`    | `UUID`        | References `workflow_runs`, `SET NULL`: the review's run that suggested it |
| `state`              | `TEXT`        | `pending`, `accepted`, or `dismissed`                                  |
| `accepted_changes`   | `INTEGER[]`   | On an accepted suggestion, the changes taken by their place from 0; `NULL` for all of them |
| `dismissal_reason`   | `TEXT`        | Why it was dismissed, 1 to 2,000 characters, which every dismissal through the API gives; only on a dismissed one |
| `decided_by`, `decided_at` | `UUID`, `TIMESTAMPTZ` | Who decided and when; set exactly once it is not `pending` |
| `created_by`         | `UUID`        | References `users`: the person, or Elysia's or a Project Manager's machine user |
| `agent`, `agent_name` | `TEXT`       | The agent acting for them: `mcp:<client id>` with the name it registered, `session:<number>`, or `project_manager:<project id>` |
| `created_at`         | `TIMESTAMPTZ` | Written by the API                                                     |

### `page_suggestion_items`

The one inbox item per page that asks a person to review its suggestions (`docs/knowledge.md`, Pages, Suggestions,
Telling people). A page has at most one at a time; when its person resolved or dismissed it by hand, the next
suggestion raises a new one and the row names it instead.

| Column           | Type          | Notes                                                                 |
|------------------|---------------|-----------------------------------------------------------------------|
| `page_id`        | `UUID`        | Primary key; references `pages`, cascading                            |
| `action_item_id` | `UUID`        | References `action_items`, cascading: the item that counts its suggestions |
| `created_by`     | `UUID`        | References `users`: the system user, since Elysium raises it          |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | `updated_at` moves when another item replaces a closed one |

### `agent_guidance_versions`

Every version of the guidance every connected agent reads, newest current; see `docs/knowledge.md`, Agent guidance. A
version is never changed: a save and a restore each add the next one, so the table is the guidance's history. Version
1 is seeded by its migration as the system user, and version 2, the default that says when to write a page, by a later
one, only where version 1 was still the only version.

| Column          | Type          | Notes                                                                 |
|-----------------|---------------|-----------------------------------------------------------------------|
| `version`       | `INTEGER`     | Primary key, from 1; the next is the current one plus 1, so two saves at once cannot both take it |
| `content`       | `TEXT`        | Markdown, trimmed, possibly empty; at most 4,096 characters here, and the API keeps it shorter (`content_max_characters`) |
| `restored_from` | `INTEGER`     | References `agent_guidance_versions`: the earlier version a restore copied; always below `version` |
| `created_by`    | `UUID`        | References `users`: the admin who saved or restored it, or the system user for version 1 |
| `created_at`    | `TIMESTAMPTZ` | Defaults to now                                                      |

### `company_doc_review_cursor`

How far the company doc's review has read the timeline (`docs/engine.md`, The company doc). One row
(`singleton`), seeded by the migration.

| Column                | Type          | Notes                                                                |
|-----------------------|---------------|----------------------------------------------------------------------|
| `singleton`           | `BOOLEAN`     | Primary key, always true                                             |
| `reviewed_through_at` | `TIMESTAMPTZ` | The ingestion time (`timeline_records.created_at`) of the last record read; NULL before the first review |
| `reviewed_through_id` | `UUID`        | That record's id, so records ingested in the same instant are each read once; not a reference, since the record may go |
| `created_by`          | `UUID`        | References `users`; the system user                                  |
| `updated_at`          | `TIMESTAMPTZ` | Maintained by trigger                                                |

### `knowledge_documents`

The knowledge base's index, one document per thing it was made from; see `docs/knowledge.md`. Each kind of source is
a column of its own, unique and cascading, with exactly one set (`knowledge_documents_one_source`): deleting the
source deletes its document and passages, a document for a source already gone is refused, and indexing a source
again upserts its one document on that column. The sources are a timeline record, a page, and a meeting's whole
transcript; `source` tells a learning and a company doc section from any other page.

| Column               | Type          | Notes                                                                  |
|----------------------|---------------|------------------------------------------------------------------------|
| `id`                 | `UUID`        | UUIDv7, primary key                                                    |
| `source`             | `TEXT`        | The search's source filter: a record's kind, such as `slack`, `learning`, `company_doc`, or `page` |
| `timeline_record_id` | `UUID`        | Unique; references `timeline_records`, cascading                       |
| `page_id`            | `UUID`        | Unique; references `pages`, cascading                                  |
| `meeting_id`         | `UUID`        | Unique; references `meetings`, cascading: a meeting's whole transcript, its `source` `meeting` |
| `title`, `url`       | `TEXT`        | How a citation names it and links to it; at most 2048 and 4096 characters |
| `occurred_at`        | `TIMESTAMPTZ` | When it was said, for the date filter                                  |
| `content_sha256`     | `TEXT`        | Hex SHA-256 of the text it was indexed from; unchanged text is left alone |
| `created_by`         | `UUID`        | References `users`: whose record it came from, for the person filter; never access control |
| `meeting_id`         | `UUID`        | References `meetings`, cascading: the meeting a segment's document is of, whose readers alone find it; `NULL` for every other source. Not a source of its own |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger              |

`knowledge_documents_copies` on `content_sha256` finds the documents holding the same text, which two people's
connectors reading one channel or space make (`docs/knowledge.md`, Hybrid search, The same text twice).
`knowledge_documents_of_meeting` finds a meeting's documents (`docs/knowledge.md`, Who sees what).

### `knowledge_asks`

Questions people ask the knowledge base for a written answer, one row each; see `docs/knowledge.md`, Asking. An ask is
its asker's alone. The API records it with what its search offered; the engine's answerer takes it, writes the answer,
and ends it. State is the enum `knowledge_ask_state`: `queued`, `running`, `answered`, or `failed`.

| Column                | Type          | Notes                                                                    |
|-----------------------|---------------|--------------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key; also the id of the turn that answers it             |
| `created_by`          | `UUID`        | References `users`: who asked, the only person who reads it back          |
| `question`            | `TEXT`        | 1 to 500 characters, also the search's query                             |
| `filters`             | `JSONB`       | An object: the filters asked for (`sources`, `project`, `person`, `from`, `to`) |
| `passages`            | `JSONB`       | An array of `{ documentId, position }`: what the search offered, in the order the prompt labels them `P1`, `P2`; references, never text |
| `searched_by_meaning` | `BOOLEAN`     | Whether the search ranked by meaning as well as by words                 |
| `state`               | `knowledge_ask_state` | Default `queued`                                                 |
| `answer`              | `JSONB`       | Set exactly when `answered`: `{ claims, sources, unsupportedClaims }`    |
| `error`               | `TEXT`        | Set exactly when `failed`, up to 2,000 characters                        |
| `created_at`          | `TIMESTAMPTZ` | When it was asked                                                        |
| `started_at`          | `TIMESTAMPTZ` | When its turn started; `NULL` for one answered without a turn            |
| `finished_at`         | `TIMESTAMPTZ` | Set exactly when `answered` or `failed`                                  |

`knowledge_asks_asked` on `(created_by, created_at DESC)` lists a person's asks and counts what they asked within the
hour; the partial `knowledge_asks_waiting` holds what the answerer has still to answer.

### `knowledge_backfills`

How far each backfill of the knowledge index has got, one row per source that needs one, seeded by its migration as
the system user: `timeline_record`, `turn_summary`, and `meeting`. See `docs/knowledge.md`, The backfill.

| Column        | Type          | Notes                                                                        |
|---------------|---------------|------------------------------------------------------------------------------|
| `source`      | `TEXT`        | Primary key: `timeline_record`, `turn_summary`, or `meeting`                  |
| `version`     | `INTEGER`     | The version of how the source is indexed (`INDEXED_AS`) the walk goes to; `NULL` before the first |
| `through_id`  | `UUID`        | The last row the walk indexed, in id order (a summary's turn id); not a reference, since the row may go |
| `finished_at` | `TIMESTAMPTZ` | When the walk reached the end; `NULL` while it is under way                  |
| `created_by`  | `UUID`        | References `users`; the system user                                          |
| `updated_at`  | `TIMESTAMPTZ` | Maintained by trigger                                                        |

### `knowledge_passages`

A document's text in order. Primary key `(document_id, position)`; cascades with its document, whose creator it
inherits. Indexing a document again rewrites its passages by position, so one whose text did not change keeps its
embedding.

| Column            | Type          | Notes                                                                    |
|-------------------|---------------|--------------------------------------------------------------------------|
| `document_id`     | `UUID`        | References `knowledge_documents`, cascading                              |
| `position`        | `INTEGER`     | From 0                                                                   |
| `content`         | `TEXT`        | 1 to 20,000 characters                                                   |
| `search_vector`   | `TSVECTOR`    | Generated: `to_tsvector('english', content)`; queries use `english` too  |
| `embedding`       | `vector(768)` | pgvector; `NULL` until the embedding model has embedded it               |
| `embedding_model` | `TEXT`        | The model and revision that made the embedding; set exactly with it      |

`knowledge_passages_search_vector` (GIN) serves full-text matches, `knowledge_passages_embedding` (HNSW,
`vector_cosine_ops`) nearest neighbours by cosine distance, the partial `knowledge_passages_unembedded` what the
embedding model has still to do, and the partial `knowledge_passages_embedding_model` the embeddings another model
made, which the engine forgets as it starts. `schema.rs` names the two column types `Tsvector` and `Vector`, placeholders
`diesel print-schema` generates.

### `pages`

The knowledge base's pages, one tree for the whole workspace; see `docs/knowledge.md`, Pages. A page holds a copy of its
newest row in `page_versions`, written together, so the tree and title searches read one table.
`pages_sibling_positions`, unique on `(parent_id, position)` with `NULLS NOT DISTINCT` for the top of the tree, keeps
siblings from tying, deleted ones included, and its index reads a parent's children in order; it is deferrable, so
renumbering them in one statement is checked when the statement ends. A live page's parent being live, and no page
being under itself at any depth, are the API's to keep, under one advisory lock (`crate::models::page`), since neither
is a rule of one row. A learning is a page with a category and evidence: the partial unique index
`pages_learning_title_unique` on `lower(title)` makes a title name one learning whatever its case, deleted learnings
included, so saving a title again revises its learning, and `migrate run` rebuilds it with every other text index when
the collation changes (Collations, above). The Learnings page, the Company page, the company doc's seven sections,
the Project briefs page, the Meetings page, and the Transcription page are seeded by migrations at ids fixed in code
(`crate::models::page`, `Section::page_id`); the sections keep their limit in `pages_company_doc_section_fits`, as a
learning keeps its in `pages_learning_body_fits`. A
project's brief is a page whose `brief_project_id` names it (`docs/autonomy.md`, The project brief), one per project,
and `pages_brief_is_a_plain_page` keeps a brief from being a learning or proposal-only. `updated_at` says when
what a page holds last changed, so its trigger skips a row whose parent or position changes: a move or a renumbering
never reorders the Learnings list.

| Column            | Type          | Notes                                                                 |
|-------------------|---------------|-----------------------------------------------------------------------|
| `id`              | `UUID`        | UUIDv7, primary key, or a fixed page's fixed id                       |
| `parent_id`       | `UUID`        | References `pages`; NULL at the top of the tree; never itself          |
| `position`        | `BIGINT`      | Its place among its siblings, lowest first, with gaps; deleted pages keep theirs |
| `title`           | `TEXT`        | 1 to 300 characters, on one line                                      |
| `body`            | `TEXT`        | Markdown, at most 100,000 characters, 20,000 on a learning and 12,000 on a company doc section; may be empty, but not on a learning |
| `category`        | `TEXT`        | References `learning_categories (key)`, on a page that is a learning; `pages_category` counts and moves them |
| `evidence`        | `TEXT`        | 1 to 20,000 characters, set with `category`; indexed after the body    |
| `current_version` | `INTEGER`     | The newest of its versions                                            |
| `proposal_only`   | `BOOLEAN`     | Agents may only suggest edits to it; people edit it directly. The Company page and the doc's sections |
| `deleted_at`      | `TIMESTAMPTZ` | Soft delete; NULL while the page is live. Descendants deleted with it share the instant, which is how restoring it finds them |
| `created_by`      | `UUID`        | References `users`: who created it                                    |
| `created_at`      | `TIMESTAMPTZ` | Written by the API, the moment it was created                         |
| `updated_at`      | `TIMESTAMPTZ` | Maintained by trigger                                                 |
| `brief_project_id` | `UUID`       | References `projects`, `SET NULL`, unique: the project whose brief it is, which makes it fixed; deleting the project leaves an ordinary page |

### `page_versions`

Every published version of a page, never changed once written; see `docs/knowledge.md`, Pages, Versions. Cascades with
its page. The key `(page_id, version)` is what stops two publishes taking one number. It repeats the agent columns of
`page_events` so a version stands alone, and holds a category as text, since a version outlives a category removed
since.

| Column                | Type          | Notes                                                             |
|-----------------------|---------------|-------------------------------------------------------------------|
| `page_id`             | `UUID`        | References `pages`, cascading                                     |
| `version`             | `INTEGER`     | 1 when created, then one more for each publish and restore        |
| `title`, `body`       | `TEXT`        | As on `pages`                                                     |
| `category`, `evidence` | `TEXT`       | As on `pages`; `category` is not a reference here                 |
| `restored_from`       | `INTEGER`     | The earlier version a restore copied                              |
| `created_by`          | `UUID`        | References `users`: who published it                              |
| `agent`, `agent_name` | `TEXT`        | The agent acting for them: `mcp:<client id>` with the name it registered, `session:<number>`, or `project_manager:<project id>` |
| `created_at`          | `TIMESTAMPTZ` | Written by the API, the moment it was published                   |

### `page_events`

Everything that happened to a page, publishes included, one row per change on each page it reached; see
`docs/knowledge.md`, Pages, History. Cascades with its page. The primary key `(page_id, id)` reads one page's history in
order, and `page_events_id` on `id` reads a feed across pages and finds a change's rows: one change to several company
doc sections is one id with a row on each.

| Column                | Type          | Notes                                                             |
|-----------------------|---------------|-------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7; orders entries as they were written, and is shared by the rows of one change |
| `page_id`             | `UUID`        | References `pages`, cascading                                     |
| `kind`                | `TEXT`        | `created`, `published`, `moved`, `deleted`, `restored`, `labeled`, `unlabeled`, `project_linked`, or `project_unlinked`; text, as for action items, so a later kind needs no enum migration |
| `created_by`          | `UUID`        | References `users`: who made the change                           |
| `agent`, `agent_name` | `TEXT`        | The agent acting for them, as on `page_versions`                  |
| `data`                | `JSONB`       | A JSON object shaped by `kind`                                    |
| `created_at`          | `TIMESTAMPTZ` | Written by the API, the moment of the change                      |

### `page_attachments`

The files attached to pages, one row each; see `docs/knowledge.md`, Pages, Attachments. Neither of its foreign keys
cascades: a row is the only record of a file kept with a provider, so a page cannot be deleted for good, nor a location
forgotten, without dealing with its files first. Rolling the migration back drops the `attached` and `detached` entries of
`page_events` with it. `page_attachments_of_page` on `(page_id, created_at)`, for the rows not removed, lists what a
page holds, and `page_attachments_location` finds what a location holds.

| Column                | Type          | Notes                                                                 |
|-----------------------|---------------|-----------------------------------------------------------------------|
| `id`                  | `UUID`        | Primary key; UUIDv7                                                   |
| `page_id`             | `UUID`        | References `pages`                                                    |
| `storage_location_id` | `UUID`        | Where this file went, through `page_attachments_storage_location`, so a location holding one stays |
| `storage_path`        | `TEXT`        | Relative to the location's directory                                  |
| `file_name`           | `TEXT`        | The name people see, cleaned; 1 to 200 characters                     |
| `content_type`        | `TEXT`        | What it is served as, from the API's table of what a page takes       |
| `size_bytes`          | `BIGINT`      | Above zero                                                            |
| `created_by`          | `UUID`        | References `users`                                                    |
| `agent`               | `TEXT`        | Nullable; the agent acting for them, as `page_versions` records it    |
| `agent_name`          | `TEXT`        | Nullable; what it called itself                                       |
| `created_at`          | `TIMESTAMPTZ` |                                                                       |
| `deleted_at`          | `TIMESTAMPTZ` | Nullable; set when it was taken off its page, its file left in place  |

### `page_labels`

The labels pages carry, one row each; see `docs/knowledge.md`, Pages, Labels. Cascades with its page, and stays while a
page is deleted softly, which is how a restored page keeps its labels. Nothing here is copied to `knowledge_documents`:
the search's label filter starts from `page_labels_label` on `(label, page_id)` and reads a document's page. What makes a
label well formed is the API's one rule (`crate::models::page_label::Label`); the table holds its length. Rolling the
migration back drops the `labeled` and `unlabeled` entries of `page_events` with it.

| Column       | Type          | Notes                                                                       |
|--------------|---------------|-----------------------------------------------------------------------------|
| `page_id`    | `UUID`        | Primary key with `label`; references `pages`, cascading                      |
| `label`      | `TEXT`        | Lowercase with hyphens for spaces; 1 to 50 characters                        |
| `created_by` | `UUID`        | References `users`: who put it on                                            |
| `created_at` | `TIMESTAMPTZ` | Written by the API, the moment it went on                                    |

### `page_projects`

Which pages are linked to which projects; see `docs/knowledge.md`, Pages, Projects. Cascades with either side, though
deleting a project takes its links out first, so each page's history records it (`crate::models::page_project`). Writes
hold the project before they lock the page, the order that deletion takes them in, so the two never wait on each other.
The primary key reads a page's projects, and `page_projects_project_id_idx` a project's pages, for its list and the
search's `project` filter.

| Column       | Type          | Notes                                                                      |
|--------------|---------------|----------------------------------------------------------------------------|
| `page_id`    | `UUID`        | References `pages`, cascading                                              |
| `project_id` | `UUID`        | References `projects`, cascading                                           |
| `created_by` | `UUID`        | References `users`: who linked it; the agent acting for them is in the page's history |
| `created_at` | `TIMESTAMPTZ` | Written by the API, the moment it was linked                               |

### `learning_categories`

The categories learnings are filed under: Elysium's nine built-in ones, seeded by the migration, and the workspace's
own, which admins add; see `docs/knowledge.md`, Categories. `pages.category` references `key` with `NO ACTION`, so
removing a category that learnings still use fails with the foreign key violation Diesel recognizes; the API moves them
first. Changes are recorded in `user_events` as a workspace setting's are. Rolling the migration back refuses while any
learning is filed under a workspace category, rather than refile or drop it.

| Column        | Type          | Notes                                                                 |
|---------------|---------------|-----------------------------------------------------------------------|
| `key`         | `TEXT`        | Primary key; a lowercase letter, then up to 49 lowercase letters, digits, or underscores; never changes |
| `label`       | `TEXT`        | 1 to 60 characters                                                    |
| `description` | `TEXT`        | 1 to 500 characters: what belongs there, which agents read            |
| `position`    | `INTEGER`     | Its place among the categories of its kind; built-in ones are offered first |
| `built_in`    | `BOOLEAN`     | One of the nine, which the API never changes or removes               |
| `created_by`  | `UUID`        | References `users`; the system user for the nine                      |
| `created_at`  | `TIMESTAMPTZ` | Set on insert                                                         |
| `updated_at`  | `TIMESTAMPTZ` | Maintained by trigger                                                 |

### `meetings`

Meetings, each the one event that happened, and how far each one's processing got; see `docs/meetings.md`. What its
source gave is its captures' (`meeting_captures`); the meeting keeps the event's own fields and what Elysium made of
it. Meetings not yet `done` are indexed for the meetings loop (`meetings_unfinished`), and meetings merged into
another for finding them (`meetings_merged_into`); a person's meetings not merged away by when they started
(`meetings_started_by_person`), for a meeting aligned to find one it may be the same as; and the meeting an inbox item
said was kept private (`meetings_visibility_items`).

| Column                   | Type          | Notes                                                                   |
|--------------------------|---------------|-------------------------------------------------------------------------|
| `id`                     | `UUID`        | UUIDv7, primary key                                                     |
| `title`                  | `TEXT`        | At most 2048 characters                                                 |
| `started_at`             | `TIMESTAMPTZ` | When it started: the zero of the meeting's clock                        |
| `duration_ms`            | `BIGINT`      | How long it ran, when its source says                                   |
| `organizer`              | `TEXT`        | Who organized it, as its source shows them                              |
| `participants`           | `JSONB`       | An array of strings                                                     |
| `segments`               | `JSONB`       | What segmenting last read of it, `{ excerpts, blackOuts }` as ranges of its words, written once per segmenting; `NULL` until it is read, and again once its person is done reviewing it |
| `hold_for_approval`      | `BOOLEAN`     | Its person asked to approve its corrected transcript before it is acted on, whatever its source's setting; `false` unless set |
| `state`                  | `TEXT`        | How far its processing got: `pending`, `recorded`, `segmented`, or `done`. Whether it failed, and why, is its run's |
| `merged_into`            | `UUID`        | References `meetings`, set to `NULL` with it: the meeting its captures were merged into, while they are; never itself (`meetings_merged_into_another`) |
| `visibility`             | `TEXT`        | Who reads it besides its owner, the people it is shared with, and admins: `unchecked` (new, private until its correction decides; the default), `private`, or `public` (every member) |
| `visibility_item_id`     | `UUID`        | References `action_items`, set to `NULL` with it: the inbox item telling its owner its correction kept it private |
| `created_by`             | `UUID`        | References `users`; whose meeting it is                                 |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger                     |

### `meeting_captures`

Each one record of a meeting, as its source gave it, kept as it came; see `docs/meetings.md`, Meetings and captures.
Its transcript and recording are written once, by its fetch, and never again. Unique on `(connector_id,
external_id)` (`meeting_captures_once_per_connector`), so a connector records each of its meetings once;
`meeting_captures_of_meeting` on `(meeting_id, joined_at, id)` reads a meeting's captures first to last.
`recording_path` and `storage_location_id` are set together (`meeting_captures_recording_fields`), and a recording's
hash only with its path (`meeting_captures_recording_hash`).

| Column                   | Type          | Notes                                                                   |
|--------------------------|---------------|-------------------------------------------------------------------------|
| `id`                     | `UUID`        | UUIDv7, primary key                                                     |
| `meeting_id`             | `UUID`        | References `meetings`, cascading                                        |
| `connector_id`           | `UUID`        | References `connectors`; set to `NULL` when the connector goes, and `NULL` for an upload |
| `source_kind`            | `TEXT`        | The source, such as `fireflies`                                         |
| `external_id`            | `TEXT`        | The source's id, at most 1024 characters; `NULL` for an upload          |
| `url`                    | `TEXT`        | Its page at the source                                                  |
| `transcript`             | `JSONB`       | An array of `{ startMs, endMs, speaker, text }`, on the capture's own clock |
| `transcript_hash`        | `TEXT`        | The SHA-256 of `transcript` as hex; `NULL` until it is fetched          |
| `storage_location_id`    | `UUID`        | References `storage_locations`: where its recording is kept             |
| `recording_path`         | `TEXT`        | The recording's path there; `NULL` when there is none                   |
| `recording_content_type` | `TEXT`        | The recording's media type                                              |
| `recording_hash`         | `TEXT`        | The SHA-256 of the recording's bytes as downloaded, as hex; `NULL` for a recording kept before captures were hashed |
| `offset_ms`              | `BIGINT`      | Where its clock falls on the meeting's: its moment `t` is `t + offset_ms` on the meeting's clock; `0` unless set |
| `offset_source`          | `TEXT`        | How `offset_ms` was found: `listed` (from when its source said it started), `audio`, `words`, or `hand` (set by a person, which aligning never changes); `listed` unless set |
| `joined_at`              | `TIMESTAMPTZ` | When it joined its meeting, its place among the meeting's captures: the one it was listed with first |
| `created_by`             | `UUID`        | References `users`; whose capture it is: the connector's person, or who uploaded it |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger                     |

### `meeting_duplicates`

Two meetings that look like one, overlapping in time with participants in common, and what their person decided; see
`docs/meetings.md`, The same meeting twice. A pair is recorded once, whichever meeting found the other
(`meeting_duplicates_once_per_pair`, unique on the pair's lesser and greater id). Indexed by each meeting and by the
inbox item asking. A decision, who made it, and when are set together (`meeting_duplicates_decided`).

| Column            | Type          | Notes                                                                        |
|-------------------|---------------|------------------------------------------------------------------------------|
| `id`              | `UUID`        | UUIDv7, primary key                                                          |
| `meeting_id`      | `UUID`        | References `meetings`, cascading: the meeting asked about, whose captures a merge moves |
| `same_as_id`      | `UUID`        | References `meetings`, cascading: the meeting it looks like, which a merge moves them into; never `meeting_id` |
| `action_item_id`  | `UUID`        | References `action_items`, set to `NULL` with it: the inbox item asking      |
| `decision`        | `TEXT`        | `merged` or `not_the_same`; `NULL` until its person decides                  |
| `decided_by`      | `UUID`        | References `users`: who decided                                              |
| `decided_at`      | `TIMESTAMPTZ` | When                                                                         |
| `created_by`      | `UUID`        | References `users`; the system user, which asks                              |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger                   |

### `meeting_uploads`

Meetings people upload, while their parts arrive and once they became a capture; see `docs/meetings.md`, Uploads. A new
meeting's upload names its title and start, and a capture's names its meeting (`meeting_uploads_new_meeting`); a
finished one names the capture or the attachment it became, never both (`meeting_uploads_finished`). `meeting_uploads_unfinished` on `(created_by,
updated_at)`, partial on what has not finished, reads a person's uploads to resume and the stale ones to sweep, and
`meeting_uploads_parts_kept` on `finished_at`, partial on finished uploads whose parts were not removed, the finished
ones to sweep.

| Column                | Type          | Notes                                                                       |
|-----------------------|---------------|-----------------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key                                                         |
| `meeting_id`          | `UUID`        | References `meetings`, cascading: the meeting a capture is added to; `NULL` for a new meeting |
| `title`, `started_at` | `TEXT`, `TIMESTAMPTZ` | A new meeting's title and when it started                           |
| `storage_location_id` | `UUID`        | References `storage_locations`: where the recording is kept; `NULL` for a transcript alone |
| `part_bytes`          | `INTEGER`     | How many bytes each part carries but the last, fixed when it starts         |
| `transcript`          | `JSONB`       | The lines its transcript was read as, `{ startMs, endMs, speaker, text }` each, set when it finishes |
| `capture_id`          | `UUID`        | References `meeting_captures`, cascading, unique: the capture it became     |
| `attachment_id`       | `UUID`        | References `meeting_attachments`, unique: the file it attached to its meeting, which it always names (`meeting_uploads_attachment_to_a_meeting`) |
| `finished_at`         | `TIMESTAMPTZ` | When it finished                                                            |
| `parts_removed_at`    | `TIMESTAMPTZ` | When its staged parts were removed from the workspace; `NULL` while they may be there |
| `created_by`          | `UUID`        | References `users`: who uploads it                                          |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger, and moved by every part, which the sweep reads |

### `meeting_upload_files`

An upload's files, a recording, a transcript, or one of each, or one file attached to a meeting; a child of its
upload, so it has no `created_by`.

| Column           | Type          | Notes                                                                            |
|------------------|---------------|----------------------------------------------------------------------------------|
| `upload_id`, `kind` | `UUID`, `TEXT` | Primary key; the upload, cascading, and `recording`, `transcript`, or `attachment` |
| `file_name`      | `TEXT`        | Its name, cleaned, at most 200 characters                                        |
| `size_bytes`     | `BIGINT`      | Above zero                                                                       |
| `modified_at`    | `TIMESTAMPTZ` | When the file last changed, as the browser read it, to resume it                 |
| `received_bytes` | `BIGINT`      | How much of it arrived, in whole parts from its start; at most its size          |

### `meeting_attachments`

Files people attach to a meeting and stills of its video pinned to it, each a frozen copy kept with the meeting; see
`docs/meetings.md`, Attachments and frames. Pins are marks of kind `pin` (`meeting_marks`) naming one in their data.
`meeting_attachments_of_meeting` on `(meeting_id, id)` lists what a meeting holds, and `meeting_attachments_location`
finds what a location holds. A frame names its capture, moment, caption, and fingerprint, and a file none of them
(`meeting_attachments_frame`).

| Column                | Type          | Notes                                                                    |
|-----------------------|---------------|--------------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key                                                      |
| `meeting_id`          | `UUID`        | References `meetings`, not cascading: the row is the only record of a file kept with a provider |
| `kind`                | `TEXT`        | `file` or `frame`                                                        |
| `storage_location_id` | `UUID`        | Where it went, through `meeting_attachments_storage_location`, so a location holding one stays |
| `storage_path`, `stored_at` | `TEXT`, `TIMESTAMPTZ` | Its path in the location and when it was stored, both `NULL` while its parts wait (`meeting_attachments_stored`) |
| `file_name`           | `TEXT`        | Its name, cleaned, at most 200 characters                                |
| `content_type`        | `TEXT`        | The type it is served as, from its extension                             |
| `size_bytes`          | `BIGINT`      | Above zero                                                               |
| `capture_id`, `capture_ms` | `UUID`, `BIGINT` | A frame's capture (references `meeting_captures`) and its moment on that capture's clock |
| `caption`             | `TEXT`        | What a frame shows, 1 to 1,000 characters                                |
| `fingerprint`         | `BYTEA`       | A frame's 32 by 18 grey cells, 576 bytes, for finding near duplicates    |
| `text`, `text_state`  | `TEXT`, `TEXT` | The text read from a file, set exactly when `text_state` is `read`; else `pending`, `none`, or `unreadable` |
| `created_by`          | `UUID`        | References `users`                                                       |
| `agent`, `agent_name` | `TEXT`        | The agent acting for its creator, as on `meeting_marks`                  |
| `created_at`          | `TIMESTAMPTZ` | Set on insert                                                            |
| `removed_by`          | `UUID`        | References `meeting_events`: the change that took it off, `NULL` while it stands |

### `meeting_transcriptions`

What Elysium's own Whisper heard in a capture's recording, one per capture, written once by the meeting workflow's
transcription step; see `docs/meetings.md`, Transcription. A table of its own beside `meeting_captures.transcript`,
because every word is timed and the meetings loop reads every unfinished meeting's capture each minute. A child of its
capture, so it has no `created_by`.

| Column            | Type          | Notes                                                                        |
|-------------------|---------------|------------------------------------------------------------------------------|
| `capture_id`      | `UUID`        | Primary key; references `meeting_captures`, cascading                        |
| `transcript`      | `JSONB`       | `{ model, language, languageProbability, durationMs, segments }`, each segment `{ startMs, endMs, text, words }` and each word `{ startMs, endMs, text, probability }` |
| `transcript_hash` | `TEXT`        | The SHA-256 of `transcript` as it was serialized to be stored, as hex        |
| `created_at`      | `TIMESTAMPTZ` | Set on insert                                                                |

### `meeting_corrections`

A meeting's corrected transcript, one per meeting: its first capture's lines with what its correction settled in
place, written once by the meeting workflow's correction step beside the raw transcripts, which it never changes, in the
transaction that settles the final transcript; see `docs/meetings.md`, Correction. A child of its meeting, so it has no
`created_by`. Approval is optional; `approved_by` and `approved_at` are set together (`meeting_corrections_approval`).

| Column                    | Type          | Notes                                                                 |
|---------------------------|---------------|-----------------------------------------------------------------------|
| `meeting_id`              | `UUID`        | Primary key; references `meetings`, cascading                         |
| `transcript`              | `JSONB`       | An array of `{ startMs, endMs, speaker, text }`, one per line of the transcript it follows, in its order, with its speakers and times |
| `transcript_hash`         | `TEXT`        | The SHA-256 of `transcript` as it was serialized to be stored, as hex |
| `based_on`                | `TEXT`        | `source` or `whisper`: the transcript it follows line by line         |
| `source_transcript_hash`, `whisper_transcript_hash` | `TEXT` | The transcripts it read; Whisper's `NULL` when there was none |
| `changed_lines`           | `INTEGER`     | How many lines it changed                                             |
| `fixes`                   | `JSONB`       | Each distinct fix its flags made, `{ heard, written }`                |
| `approved_by`             | `UUID`        | References `users`: who approved it                                   |
| `approved_at`             | `TIMESTAMPTZ` | When                                                                  |
| `review_item_id`          | `UUID`        | References `action_items`, set to `NULL` with it: the inbox item raised when its run held for approval; indexed, for the item's page |
| `created_at`              | `TIMESTAMPTZ` | Set on insert                                                         |

### `meeting_events`

A meeting's history, one row per change, shaped like `page_events`; see `docs/meetings.md`, History. Cascades with its
meeting; `meeting_events_of_meeting` on `(meeting_id, id)` reads one meeting's history in order.

| Column                | Type          | Notes                                                             |
|-----------------------|---------------|-------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key; orders entries as they were written          |
| `meeting_id`          | `UUID`        | References `meetings`, cascading                                  |
| `kind`                | `TEXT`        | `built`, `settled`, `edited`, `marked`, `unmarked`, `hold_changed`, `correction_approved`, `offset_changed`, `merged`, `split`, `capture_added`, `thread_resolved`, or `thread_reopened`; text, so a later kind needs no migration |
| `created_by`          | `UUID`        | References `users`: who made the change, a person or the system   |
| `agent`, `agent_name` | `TEXT`        | The agent acting for them, as on `page_events`                    |
| `data`                | `JSONB`       | A JSON object shaped by `kind`, and `undid` for an undo           |
| `created_at`          | `TIMESTAMPTZ` | Written by the API, the moment of the change                      |

### `meeting_words`

A meeting's final transcript: every word it ever held, standing or replaced, each with a stable id; see
`docs/meetings.md`, The final transcript. Cascades with its meeting. `position` orders every word of a meeting in one
sequence, gapped by 1024, and is unique per meeting (`meeting_words_order`, deferrable so one statement renumbers a
meeting); `meeting_words_standing` on `(meeting_id, position)`, where `superseded_by` is `NULL`, reads the transcript
as it reads now. A word's span never runs backwards (`meeting_words_span`).

| Column          | Type          | Notes                                                                      |
|-----------------|---------------|----------------------------------------------------------------------------|
| `id`            | `UUID`        | UUIDv7, primary key; what marks anchor to                                  |
| `meeting_id`    | `UUID`        | References `meetings`, cascading                                           |
| `position`      | `BIGINT`      | Its place in the sequence                                                  |
| `text`          | `TEXT`        | The word as written, with no space around it; never empty                  |
| `speaker`       | `TEXT`        | Who said it, as its capture names them; empty when nothing says            |
| `start_ms`, `end_ms` | `BIGINT` | On the meeting's clock                                                     |
| `origin`        | `TEXT`        | `source`, `whisper`, `correction`, or `edit`: where it came from           |
| `capture_id`    | `UUID`        | References `meeting_captures`, set to `NULL` with it: the capture it was heard in, or whose words a correction settled; `NULL` for an edit's words |
| `introduced_by` | `UUID`        | References `meeting_events`: the change that wrote it                      |
| `superseded_by` | `UUID`        | References `meeting_events`: the change that replaced it; `NULL` while it stands |
| `created_at`    | `TIMESTAMPTZ` | Set on insert                                                              |

### `meeting_marks`

Everything anchored to a range of a meeting's words: confidence flags, edits, excerpts, black-outs, comments, pins,
and tasks; see `docs/meetings.md`, The final transcript. Cascades with its meeting; `meeting_marks_of_meeting` on
`(meeting_id, id)` reads one meeting's. Both ends stand: an edit moves every mark over the words it replaces onto the
words that replace them.

| Column                | Type          | Notes                                                             |
|-----------------------|---------------|-------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key                                               |
| `meeting_id`          | `UUID`        | References `meetings`, cascading                                  |
| `kind`                | `TEXT`        | `confidence`, `edit`, `excerpt`, `black_out`, `comment`, `pin`, or `task`; text, so a later kind needs no migration |
| `start_word_id`, `end_word_id` | `UUID` | Reference `meeting_words`: the first and last word of its range |
| `data`                | `JSONB`       | A JSON object shaped by `kind`: a confidence mark's `{ confidence, alternatives }`; an excerpt's and a black-out's `{}`, an excerpt's projects being its record's |
| `created_by`          | `UUID`        | References `users`: who made it                                   |
| `agent`, `agent_name` | `TEXT`        | The agent acting for them, as on `meeting_events`                 |
| `created_at`          | `TIMESTAMPTZ` | Set on insert                                                     |
| `removed_by`          | `UUID`        | References `meeting_events`: the change that removed it; `NULL` while it stands |

A comment mark is a thread, its data `{ resolved, resolution }`: whether it is resolved, and the history entry that
last resolved or reopened it, which is the one an undo may reverse.

### `meeting_comments`

What is written in a meeting's comment threads; see `docs/meetings.md`, Comments. A thread is its comment mark, so
a comment cascades with it; removing the mark keeps its comments, so an undo puts the thread back whole.
`meeting_comments_of_thread` on `(mark_id, id)` reads one thread in order.

| Column                | Type          | Notes                                                             |
|-----------------------|---------------|-------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key; orders a thread's comments as they were written |
| `mark_id`             | `UUID`        | References `meeting_marks`, cascading: its thread                 |
| `body`                | `TEXT`        | Plain text as written, 1 to 20,000 characters, never blank        |
| `created_by`          | `UUID`        | References `users`: a person, or Elysia answering in the thread   |
| `agent`, `agent_name` | `TEXT`        | The agent acting for them, as on `meeting_marks`                  |
| `created_at`          | `TIMESTAMPTZ` | Written by the API, the moment it was written                     |

### `meeting_comment_mentions`

Whom each comment mentions, primary key `(comment_id, user_id)`: each person is told with an inbox item, and Elysia
answers in the thread. Its author is the comment's.

| Column       | Type   | Notes                                              |
|--------------|--------|----------------------------------------------------|
| `comment_id` | `UUID` | References `meeting_comments`, cascading           |
| `user_id`    | `UUID` | References `users`: an active person, or Elysia    |

### `meeting_shares`

Whom a meeting is shared with: a private meeting is read by its owner and these people, and a guest reads only a meeting
shared with them, public or not (`docs/meeting-review.md`, Who sees a meeting). `meeting_shares_of_person` finds the
meetings shared with a person, which every listing and search of meetings asks.

| Column       | Type          | Notes                                                                       |
|--------------|---------------|-----------------------------------------------------------------------------|
| `meeting_id` | `UUID`        | References `meetings`, cascading; with `person_id`, the primary key          |
| `person_id`  | `UUID`        | References `users`: an active person other than its owner when shared        |
| `created_by` | `UUID`        | References `users`: its owner, an admin, or the system for its correction's decision |
| `created_at` | `TIMESTAMPTZ` | Set on insert                                                                |

### `meeting_thread_items`

The inbox items a thread raised, one per person a mention, a reply, a resolve, or a reopen told, so an item's page
opens the thread it is about.

| Column           | Type          | Notes                                                        |
|------------------|---------------|--------------------------------------------------------------|
| `action_item_id` | `UUID`        | Primary key; references `action_items`, cascading            |
| `mark_id`        | `UUID`        | References `meeting_marks`, cascading: the thread            |
| `person_id`      | `UUID`        | References `users`: who it tells                             |
| `created_by`     | `UUID`        | References `users`: whose change raised it                   |
| `created_at`     | `TIMESTAMPTZ` | Set on insert                                                |

### `meeting_tasks`

The tasks heard in a meeting, one per mark of kind `task` (`docs/meetings.md`, Tasks): the mark anchors it to its words,
and this row says what it is.

| Column            | Type          | Notes                                                                        |
|-------------------|---------------|------------------------------------------------------------------------------|
| `mark_id`         | `UUID`        | Primary key; references `meeting_marks`, cascading: the task's id            |
| `meeting_id`      | `UUID`        | References `meetings`, cascading; indexed                                    |
| `title`           | `TEXT`        | 1 to 200 characters: what is to be done                                      |
| `owed_by_user_id` | `UUID`        | References `users`: a person on Elysium who owes it                          |
| `owed_by_name`    | `TEXT`        | 1 to 200 characters: someone not on Elysium who owes it, by the name heard; never beside `owed_by_user_id` (`meeting_tasks_owed_by_one`), and neither means nobody was named |
| `project_id`      | `UUID`        | References `projects`, set null on delete: the project whose work it is; indexed while set |
| `declined_reason` | `TEXT`        | 1 to 2,000 characters: why its Project Manager made no item of it            |
| `declined_at`     | `TIMESTAMPTZ` | Set exactly with the reason (`meeting_tasks_declined_whole`)                 |
| `created_by`      | `UUID`        | References `users`: who heard it, Elysia                                     |
| `created_at`      | `TIMESTAMPTZ` | Set on insert                                                                |

### `meeting_task_items`

The action items a task was made into, so the meeting's bar opens them and each item's page opens its task's words.

| Column           | Type          | Notes                                                                   |
|------------------|---------------|-------------------------------------------------------------------------|
| `action_item_id` | `UUID`        | Primary key; references `action_items`, cascading                       |
| `task_id`        | `UUID`        | References `meeting_tasks`, cascading; indexed                          |
| `person_id`      | `UUID`        | References `users`: whom an item made at once is for; null for a project's item, which its Project Manager made |
| `created_by`     | `UUID`        | References `users`: Elysia for an item made at once, the proposer for a changeset's |
| `created_at`     | `TIMESTAMPTZ` | Set on insert                                                           |

### `mail_servers`

The mail server Elysium runs, with the administrator Stalwart issued when it was created; see `docs/mail.md`. At most
one row, enforced by the unique index `mail_servers_singleton` on `(true)`.

| Column                   | Type          | Notes                                                    |
|--------------------------|---------------|----------------------------------------------------------|
| `id`                     | `UUID`        | UUIDv7, primary key                                      |
| `hostname`               | `TEXT`        | The name the server answers as, lowercase                |
| `admin_username`         | `TEXT`        | Stalwart's administrator, `admin@<first domain>`         |
| `admin_secret_encrypted` | `BYTEA`       | Sealed administrator password, see `docs/secrets.md`     |
| `created_at`             | `TIMESTAMPTZ` | Set on insert                                            |

### `mail_domains`

Domains the mail server hosts.

| Column        | Type          | Notes                                                                 |
|---------------|---------------|-----------------------------------------------------------------------|
| `id`          | `UUID`        | UUIDv7, primary key                                                   |
| `name`        | `TEXT`        | Lowercase, unique                                                     |
| `stalwart_id` | `TEXT`        | Stalwart's id for the domain                                          |
| `is_default`  | `BOOLEAN`     | The domain the server was created with; at most one, never removed    |
| `created_at`  | `TIMESTAMPTZ` | Set on insert                                                         |

### `projects`

What coding sessions are grouped under.

| Column        | Type          | Notes                                    |
|---------------|---------------|------------------------------------------|
| `id`          | `UUID`        | UUIDv7, primary key                      |
| `name`        | `TEXT`        | 1 to 120 characters, unique              |
| `description` | `TEXT`        | Up to 2000 characters, defaults to empty |
| `created_at`  | `TIMESTAMPTZ` | Set on insert                            |
| `updated_at`  | `TIMESTAMPTZ` | Maintained by trigger                    |
| `cover_image` | `BYTEA`       | Cover, compressed to WebP; at most 1 MB stored |
| `cover_image_updated_at` | `TIMESTAMPTZ` | When the cover changed; set exactly when `cover_image` is |
| `cover_removed_at` | `TIMESTAMPTZ` | When the cover was removed; it stays stored to restore |
| `cover_fit`   | `project_cover_fit` | `fit` or `fill`, defaults to `fit`; kept when the cover is removed |
| `github_access` | `github_access` | `default`, `none`, or `specific`; defaults to `default` |
| `github_credential_id` | `UUID` | References `github_credentials`, `ON DELETE SET NULL`; set only for `specific` |
| `tools_setup_script` | `TEXT` | Nullable, 1 to 20,000 characters: bash run as root in every worker's container before its repositories are cloned |
| `autonomy` | `project_autonomy` | Its trust dial: `approve_every_batch` (the default), `approve_found_work`, or `full_autonomy`; changed only through `project_autonomy::set` |

`Project` in `api/src/models/project.rs` leaves `cover_image` out, so listing projects never loads images;
`find_cover` reads it for the one route that serves a cover. A removed cover is neither served nor reported:
`Project::cover_updated_at` is the one that shows. It leaves `tools_setup_script` out too, which only workers and the
project's Workers page read (`api/src/models/worker_setup.rs`).

`autonomy` says what of the project's Project Manager's work waits for a person (`docs/autonomy.md`, The dial): a
human-rooted batch is submitted `queued` above the default, its changes apply at once only at `full_autonomy`, and a
found batch is proposed at every level. Every project starts at the default.

### `project_autonomy_changes`

Each turn of a project's trust dial, by the person who turned it (`api/src/models/project_autonomy.rs`). The newest is
who set the level the project has; a project with none is still at the default. Written in the transaction that
changes `projects.autonomy`, under the project's row lock.

| Column              | Type               | Notes                                                       |
|---------------------|--------------------|-------------------------------------------------------------|
| `id`                | `UUID`             | UUIDv7, primary key, so a project's changes order by it     |
| `project_id`        | `UUID`             | References `projects`, cascading                            |
| `previous_autonomy` | `project_autonomy` | The level it moved from                                     |
| `autonomy`          | `project_autonomy` | The level it moved to; differs from `previous_autonomy`     |
| `created_by`        | `UUID`             | References `users`: the company member who turned it        |
| `created_at`        | `TIMESTAMPTZ`      | Set on insert                                               |

Indexed by `(project_id, id DESC)` for a project's newest change.

### `project_repositories`

The repositories a project declares for its workers, in order, each with what readies it once cloned; see
`docs/engine.md`, Workers. Replaced whole by `worker_setup::replace`, so the position is unique per project only at
commit (deferred), and two repositories may swap places in one save.

| Column         | Type          | Notes                                                                 |
|----------------|---------------|-----------------------------------------------------------------------|
| `id`           | `UUID`        | UUIDv7, primary key                                                   |
| `project_id`   | `UUID`        | References `projects`, cascading                                      |
| `position`     | `INTEGER`     | From 0; unique per project                                            |
| `url`          | `TEXT`        | 1 to 2,048 characters, held to a coding session's rules by the API    |
| `base_branch`  | `TEXT`        | Nullable, 1 to 255 characters; the remote's default when unset        |
| `setup_script` | `TEXT`        | Nullable, 1 to 20,000 characters: bash run as the agent in its directory once cloned |
| `merge_policy` | `merge_policy` | `person` (the default) or `auto`: who merges its workers' pull requests (`docs/engine.md`, Merging); `auto` only on github.com, held to it by the API |
| `created_by`   | `UUID`        | References `users`: who saved the setup                               |
| `created_at`   | `TIMESTAMPTZ` | Set on insert                                                         |

### `project_issue_trackers`

The tracker a project names for the issues its Project Manager files, overriding its precedent; see `docs/engine.md`,
Its writes. One row per project at most (`project_id`, the primary key, deleted with the project).

| Column               | Type            | Notes                                                              |
|----------------------|-----------------|--------------------------------------------------------------------|
| `provider`           | `link_provider` | `jira`, `github`, or `youtrack`                                    |
| `jira_credential_id` | `UUID`          | Set exactly for `jira`; deleted with the credential               |
| `token_account_id`   | `UUID`          | Set exactly for `youtrack`; deleted with the account              |
| `destination`        | `TEXT`          | A Jira project key, a GitHub `owner/name`, or a YouTrack project's short name, as the provider names it |
| `created_by`         | `UUID`          | The person who named it                                            |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | `updated_at` maintained by trigger                           |

A GitHub tracker names no credential: it files with the token the project's sessions get.

### `github_credentials`

The GitHub tokens Elysium holds; see `docs/github.md`.

| Column             | Type                 | Notes                                                       |
|--------------------|----------------------|-------------------------------------------------------------|
| `id`               | `UUID`               | UUIDv7, primary key                                         |
| `name`             | `TEXT`               | 1 to 120 characters, unique                                 |
| `kind`             | `github_token_kind`  | `classic` or `fine_grained`                                 |
| `token_encrypted`  | `BYTEA`              | Sealed token, see `docs/secrets.md`                         |
| `login`            | `TEXT`               | The account the token acts as: up to 39 letters, digits, hyphens |
| `scopes`           | `TEXT`               | Comma separated, as GitHub lists them; empty for fine-grained |
| `token_expires_at` | `TIMESTAMPTZ`        | NULL for a token that does not expire                       |
| `checked_at`       | `TIMESTAMPTZ`        | When GitHub last confirmed the token                        |
| `is_default`       | `BOOLEAN`            | The workspace default; a partial unique index allows one    |
| `created_at`       | `TIMESTAMPTZ`        | Set on insert                                               |
| `updated_at`       | `TIMESTAMPTZ`        | Maintained by trigger                                       |

Every column from `login` on is what GitHub answered, so a row is only written after GitHub accepts the token.

### `issue_reports`

Where each problem report went on Elysium's GitHub repository; see `docs/issue-reporting.md`. A row is written only
once GitHub accepted the issue or comment, and nothing that was posted is kept: GitHub holds it.

| Column         | Type                  | Notes                                                        |
|----------------|-----------------------|--------------------------------------------------------------|
| `id`           | `UUID`                | UUIDv7, primary key                                          |
| `fingerprint`  | `TEXT`                | The problem: 16 lower-case hex characters, checked in SQL    |
| `issue_number` | `BIGINT`              | The issue it went to, greater than zero                      |
| `issue_url`    | `TEXT`                | That issue on `https://github.com/`                          |
| `filed_as`     | `issue_report_filing` | `issue` when it opened the issue, `comment` when it joined one |
| `url`          | `TEXT`                | Where this report reads: the issue, or its comment           |
| `created_by`   | `UUID`                | The reporter                                                 |
| `created_at`   | `TIMESTAMPTZ`         | Set on insert                                                |

It is indexed by reporter and time, for the daily limit, and by fingerprint and time, for the issue a problem last went
to, which is found before GitHub's search has indexed it.

### `jira_credentials`

The Atlassian accounts Elysium holds, each carrying exactly one connector: a Jira connector, or a Confluence connector
whose account allows no Jira project or board. Everything that lists Jira sites lists only the first; see
`docs/jira.md` and `docs/connectors.md`.

| Column            | Type          | Notes                                                                 |
|-------------------|---------------|-----------------------------------------------------------------------|
| `id`              | `UUID`        | UUIDv7, primary key                                                   |
| `name`            | `TEXT`        | 1 to 120 characters, unique                                           |
| `site_url`        | `TEXT`        | The site's origin; https on `.atlassian.net`, with no port or path    |
| `account_email`   | `TEXT`        | The Atlassian account, the username half of basic auth; up to 320     |
| `token_encrypted` | `BYTEA`       | Sealed API token, see `docs/secrets.md`                               |
| `account_id`      | `TEXT`        | The account Jira reports for the token                                |
| `display_name`    | `TEXT`        | That account's name, up to 255 characters, defaults to empty          |
| `all_projects`    | `BOOLEAN`     | Every project, including ones added later; defaults to false          |
| `all_boards`      | `BOOLEAN`     | Every board, the same way                                             |
| `checked_at`      | `TIMESTAMPTZ` | When Jira last confirmed the token                                    |
| `created_at`      | `TIMESTAMPTZ` | Set on insert                                                         |
| `updated_at`      | `TIMESTAMPTZ` | Maintained by trigger                                                 |

`account_id` and `display_name` are what Jira answered, so a row is only written after Jira accepts the token.

### `jira_credential_projects`

The projects a credential may touch without `all_projects`. The primary key is `(jira_credential_id, project_id)`,
and `(jira_credential_id, project_key)` is unique as well, since the key is what a bounded search names in JQL.

| Column               | Type          | Notes                                                             |
|----------------------|---------------|-------------------------------------------------------------------|
| `jira_credential_id` | `UUID`        | References `jira_credentials`; deleted with the credential        |
| `project_id`         | `TEXT`        | Jira's own project id, digits only                                |
| `project_key`        | `TEXT`        | 2 to 255 characters: a letter, then uppercase letters, digits, `_` |
| `name`               | `TEXT`        | The project's name, so a selection renders without calling Jira   |
| `created_at`         | `TIMESTAMPTZ` | Set on insert                                                     |

### `jira_credential_boards`

The boards a credential may touch without `all_boards`. The primary key is `(jira_credential_id, board_id)`.

| Column               | Type          | Notes                                                        |
|----------------------|---------------|--------------------------------------------------------------|
| `jira_credential_id` | `UUID`        | References `jira_credentials`; deleted with the credential   |
| `board_id`           | `BIGINT`      | Jira's own board id, greater than zero                       |
| `name`               | `TEXT`        | The board's name                                             |
| `project_key`        | `TEXT`        | The board's project when it has exactly one; NULL otherwise  |
| `created_at`         | `TIMESTAMPTZ` | Set on insert                                                |

### `environment_variables`

Global environment variables every coding session's turns receive; see `docs/environment.md`.

| Column            | Type          | Notes                                                                  |
|-------------------|---------------|------------------------------------------------------------------------|
| `id`              | `UUID`        | UUIDv7, primary key                                                    |
| `key`             | `TEXT`        | Unique; `^[A-Za-z_][A-Za-z0-9_]*$`, up to 128 characters               |
| `value_encrypted` | `BYTEA`       | Sealed value, secret or not, see `docs/secrets.md`                     |
| `is_secret`       | `BOOLEAN`     | The value is write-only over HTTP and scrubbed from session events     |
| `description`     | `TEXT`        | Up to 500 characters, defaults to empty                                |
| `created_at`      | `TIMESTAMPTZ` | Set on insert                                                          |
| `updated_at`      | `TIMESTAMPTZ` | Maintained by trigger                                                  |

Which keys may be stored beyond their shape is decided in the API, not in SQL, since the rules follow the runner.

### `storage_locations`

External locations Elysium saves files to; see `docs/storage.md`.

| Column                 | Type                    | Notes                                                        |
|------------------------|-------------------------|--------------------------------------------------------------|
| `id`                   | `UUID`                  | UUIDv7, primary key                                          |
| `name`                 | `TEXT`                  | 1 to 120 characters, unique                                  |
| `kind`                 | `storage_location_kind` | The provider: `bunny` or `s3`                                |
| `bunny_zone`           | `TEXT`                  | Set exactly for `bunny`; letters, digits, hyphens, up to 64  |
| `bunny_region`         | `bunny_storage_region`  | Set exactly for `bunny`                                      |
| `path_prefix`          | `TEXT`                  | Up to 1024 characters, no leading, trailing, or double slash |
| `storage_limit_bytes`  | `BIGINT`                | Greater than zero; NULL for no limit                         |
| `access_key_encrypted` | `BYTEA`                 | Sealed access key, see `docs/secrets.md`                     |
| `created_at`           | `TIMESTAMPTZ`           | Set on insert                                                |
| `updated_at`           | `TIMESTAMPTZ`           | Maintained by trigger                                        |
| `all_projects`         | `BOOLEAN`               | Every project saves files here; defaults to false            |
| `s3_service`           | `s3_service`            | Set exactly for `s3`: `aws` or `google_cloud`                |
| `s3_bucket`            | `TEXT`                  | Set exactly for `s3`                                         |
| `s3_region`            | `TEXT`                  | Set exactly for `aws`                                        |
| `s3_access_key_id`     | `TEXT`                  | Set exactly for `s3`; letters and digits, up to 256          |
| `is_studio_default`    | `BOOLEAN`               | Studio's New item form starts here; at most one row is true  |

`storage_locations_one_studio_default_idx`, a partial unique index, holds the one default.

Constraints compare `kind` as text, because the migration adding `s3` can share a transaction with the one that uses
it, and Postgres refuses a new enum value as a literal in the transaction that added it.

### `storage_location_projects`

The projects that save files to a location without `all_projects`. The primary key is `(storage_location_id,
project_id)`, and an index on `project_id` finds a project's locations.

| Column                | Type          | Notes                                                        |
|-----------------------|---------------|--------------------------------------------------------------|
| `storage_location_id` | `UUID`        | References `storage_locations`; deleted with the location    |
| `project_id`          | `UUID`        | References `projects`; deleted with the project              |
| `created_at`          | `TIMESTAMPTZ` | Set on insert                                                |

### `coding_sessions`

An agent conversation: a directory in the workspace, the Claude session its turns resume, and the events kept in
`session_events`; see `docs/coding.md`.

| Column         | Type          | Notes                                                                 |
|----------------|---------------|-----------------------------------------------------------------------|
| `id`           | `BIGINT`      | Identity from 1; the API reserves it with `nextval` before the row exists |
| `project_id`   | `UUID`        | References `projects`; a project with sessions cannot be deleted. NULL only for a Studio session |
| `title`        | `TEXT`        | 1 to 200 characters                                                   |
| `github_credential_id` | `UUID` | The GitHub token its turns use; `ON DELETE SET NULL`               |
| `action_item_id` | `UUID`      | The action item the session was started from; `ON DELETE SET NULL`   |
| `studio_item_id` | `UUID`      | The Studio item it works on; NULL for a Coding session. Deleted with the item |
| `harness_session_id` | `TEXT`  | The Claude session the next turn resumes; 1 to 200 characters, NULL until a turn reports one |
| `created_at`   | `TIMESTAMPTZ` | Set on insert                                                         |
| `updated_at`   | `TIMESTAMPTZ` | Maintained by trigger                                                 |

`coding_sessions_project_required` holds that only a Studio session may lack a project.
`coding_sessions_created_at_idx` serves the newest-first overview, `coding_sessions_project_id_idx` the project
lookups, including the delete check, `coding_sessions_action_item_id_idx` an item's sessions, and
`coding_sessions_studio_item_id_idx` a Studio item's. Action items are deleted softly, so a session keeps its item
while the item is hidden.

The `project_id` foreign key is `ON DELETE NO ACTION` rather than `RESTRICT`. Both refuse the delete, but only
`NO ACTION` reports SQLSTATE `23503`, which Diesel surfaces as a foreign key violation; `RESTRICT` reports
`23001`, which Diesel does not classify.

### `session_events`

Every event a session's turns reported, scrubbed of the turn's secrets. The primary key is `(session_id, sequence)`.

| Column        | Type          | Notes                                                    |
|---------------|---------------|----------------------------------------------------------|
| `session_id`  | `BIGINT`      | References `coding_sessions`; deleted with the session   |
| `sequence`    | `BIGINT`      | Counts from 1 per session; not negative                  |
| `turn_id`     | `UUID`        | The turn that reported it                                |
| `occurred_at` | `TIMESTAMPTZ` | When Elysium received it                                 |
| `payload`     | `JSONB`       | The runner's event (`RunnerEvent`), rendered when read   |

`session_events_finished_at` indexes `occurred_at` over the `finished` events alone, which carry each turn's cost
and tokens, so the model settings pages total a window of turns without reading every event (`docs/engine.md`,
Models).

### `turn_summaries`

A finished turn's one or two sentence summary; see `docs/coding.md`, Turn summaries. A child of `coding_sessions`, so
it inherits its session's creator. The primary key is `(session_id, turn_id)`; recording again replaces the summary.
`turn_summaries_turn_id` serves the backfill's walk in turn order. A summary of a session with a project has a record on
that project's timeline (`timeline_records.turn_summary_*`), deleted with it.

| Column       | Type          | Notes                                                    |
|--------------|---------------|----------------------------------------------------------|
| `session_id` | `BIGINT`      | References `coding_sessions`; deleted with the session   |
| `turn_id`    | `UUID`        | The turn it summarizes                                   |
| `summary`    | `TEXT`        | 1 to 500 characters; Elysium keeps at most 400           |
| `created_at` | `TIMESTAMPTZ` | Set on insert                                            |

### `studio_items`

Studio's single assets; see `docs/studio.md`.

| Column                | Type          | Notes                                                              |
|-----------------------|---------------|--------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key                                                |
| `title`               | `TEXT`        | 1 to 200 characters                                                |
| `prompt`              | `TEXT`        | The first prompt; 1 to 100,000 characters                          |
| `project_id`          | `UUID`        | Optional; references `projects`, `ON DELETE NO ACTION`             |
| `storage_location_id` | `UUID`        | References `storage_locations`, `ON DELETE NO ACTION`              |
| `thumbnail_asset_id`  | `UUID`        | The pinned thumbnail; references `studio_assets`, `ON DELETE SET NULL` |
| `pull_error`          | `TEXT`        | Why the latest file could not be kept; cleared by the next that is |
| `deleted_at`          | `TIMESTAMPTZ` | Set by a soft delete                                               |
| `created_at`          | `TIMESTAMPTZ` | Set on insert                                                      |
| `updated_at`          | `TIMESTAMPTZ` | Maintained by trigger                                              |

`studio_items_live_created_at_idx` serves the grid; the project and location indexes serve the delete checks.

### `studio_assets`

One version of one file an agent delivered. `studio_assets_version_unique` on `(studio_item_id, artifact_path,
sha256)` makes a pull of a file already kept a no-op.

| Column           | Type                | Notes                                                        |
|------------------|---------------------|--------------------------------------------------------------|
| `id`             | `UUID`              | UUIDv7, primary key                                          |
| `studio_item_id` | `UUID`              | References `studio_items`; deleted with the item             |
| `session_id`     | `BIGINT`            | The session it came from; `ON DELETE SET NULL`               |
| `kind`           | `studio_asset_kind` | `image`, `model`, or `file`                                  |
| `artifact_path`  | `TEXT`              | Where the agent wrote it, relative to `artifacts/`           |
| `content_type`   | `TEXT`              | From the file's extension, when it is known                  |
| `size_bytes`     | `BIGINT`            | Not negative                                                 |
| `sha256`         | `TEXT`              | 64 lowercase hex characters                                  |
| `storage_path`   | `TEXT`              | Where it is kept: `studio/<item>/<sha256>.<extension>`, shared by every row with those bytes |
| `created_at`     | `TIMESTAMPTZ`       | Set on insert                                                |

### `studio_storage_reservations`

Bytes promised to a Studio upload in flight into a location with a storage limit, taken under an advisory lock per
location (`api/src/models/studio_reservation.rs`) so concurrent uploads cannot pass the limit together. A row lives for
one upload; one older than an hour no longer counts and is cleared by the next release.

| Column                | Type          | Notes                                                              |
|-----------------------|---------------|--------------------------------------------------------------------|
| `id`                  | `UUID`        | UUIDv7, primary key                                                |
| `storage_location_id` | `UUID`        | References `storage_locations`; deleted with it                    |
| `size_bytes`          | `BIGINT`      | Not negative                                                       |
| `created_by`          | `UUID`        | The coding agent for a delivered file, the person for a drawing    |
| `created_at`          | `TIMESTAMPTZ` | Set on insert                                                      |

### `studio_feedback`

A prompt sent with a drawing. Both images are kept in the item's storage location.

| Column                   | Type          | Notes                                                    |
|--------------------------|---------------|----------------------------------------------------------|
| `id`                     | `UUID`        | UUIDv7, primary key                                      |
| `studio_item_id`         | `UUID`        | References `studio_items`; deleted with the item         |
| `session_id`             | `BIGINT`      | The session it was sent to; `ON DELETE SET NULL`         |
| `turn_id`                | `TEXT`        | The turn it started; NULL until the turn is queued       |
| `prompt`                 | `TEXT`        | 1 to 100,000 characters                                  |
| `source_asset_id`        | `UUID`        | What was drawn over; `ON DELETE SET NULL`                |
| `camera_orbit`           | `TEXT`        | The 3D viewer's camera at capture; up to 200 characters  |
| `annotated_storage_path` | `TEXT`        | The drawing as sent                                      |
| `annotated_size_bytes`   | `BIGINT`      |                                                          |
| `capture_storage_path`   | `TEXT`        | The view without the drawing; set with its size          |
| `capture_size_bytes`     | `BIGINT`      |                                                          |
| `created_at`             | `TIMESTAMPTZ` | Set on insert                                            |

### `action_items`

What the user owes attention to; see `docs/action-items.md`.

| Column          | Type                     | Notes                                                        |
|-----------------|--------------------------|--------------------------------------------------------------|
| `id`            | `UUID`                   | UUIDv7, primary key                                          |
| `title`         | `TEXT`                   | 1 to 500 characters                                          |
| `notes`         | `TEXT`                   | Up to 20,000 characters, defaults to empty                   |
| `state`         | `action_item_state`      | `inbox`, `open`, `resolved`, or `dismissed`                  |
| `priority`      | `action_item_priority`   | `urgent`, `high`, `normal`, `low`, declared in that order; defaults to `normal` |
| `due_at`        | `TIMESTAMPTZ`            | Optional                                                     |
| `snoozed_until` | `TIMESTAMPTZ`            | Hidden from Next until then                                  |
| `waiting_on`    | `TEXT`                   | Who owes the next step, 1 to 320 characters; NULL when nobody |
| `owner_kind`    | `action_item_owner_kind` | `user`, `other`, or `nobody`; defaults to `user`             |
| `owner_name`    | `TEXT`                   | 1 to 320 characters; set exactly when `owner_kind` is `other` |
| `resolved_at`   | `TIMESTAMPTZ`            | Set exactly while `state` is `resolved`                      |
| `dismissed_at`  | `TIMESTAMPTZ`            | Set exactly while `state` is `dismissed`                     |
| `deleted_at`    | `TIMESTAMPTZ`            | Soft delete; NULL while the item is live                     |
| `created_at`    | `TIMESTAMPTZ`            | Written by the API, the moment of the create                 |
| `updated_at`    | `TIMESTAMPTZ`            | Maintained by trigger                                        |
| `lead_project_id` | `UUID`                 | The one of its projects that leads it; set exactly while it is in any |
| `manager_assigned_at` | `TIMESTAMPTZ`      | When a person assigned it to its lead project's Project Manager; NULL while it is not |
| `manager_assigned_by` | `UUID`             | References `users`: the person who assigned it, set exactly with `manager_assigned_at` |

The partial index `action_items_live_state_idx` on `state` where `deleted_at` is NULL serves Next and the state
filters. `action_items_lead_project_is_a_membership` makes `(id, lead_project_id)` reference `action_item_projects`,
checked at commit so an item is created with its lead before its memberships, and set to NULL should a membership go
without the code moving the lead first, which it always does, recording the move. Moving the lead clears the
assignment in the same statement (`action_item::move_lead`), so an assignment is always its lead's; no check holds it
to a lead, because taking an item out of its lead project nulls the lead at once, before the move in the same
transaction ends the assignment. `action_items_manager_assignment_whole` keeps its two columns set together.

### `action_item_projects`

The projects an item belongs to. The primary key is `(action_item_id, project_id)`, and an index on `project_id`
finds a project's items. Both references delete their rows with the item or the project.

| Column           | Type          | Notes                                                                     |
|------------------|---------------|---------------------------------------------------------------------------|
| `action_item_id` | `UUID`        | References `action_items`                                                 |
| `project_id`     | `UUID`        | References `projects`                                                     |
| `confidence`     | `REAL`        | 0 to 1: how sure whoever put it there was; 1 for anyone but the attribution step |
| `attributed_by`  | `TEXT`        | The actor, as history names it (`user:<id>`, `elysia`, `project_manager:<id>`), 1 to 100 characters |
| `created_at`     | `TIMESTAMPTZ` | When it joined; breaks a tie in confidence when the lead passes on        |

### `initiatives`

| Column        | Type               | Notes                                            |
|---------------|--------------------|--------------------------------------------------|
| `id`          | `UUID`             | UUIDv7, primary key                              |
| `name`        | `TEXT`             | 1 to 200 characters                              |
| `description` | `TEXT`             | Up to 20,000 characters, defaults to empty       |
| `target_at`   | `TIMESTAMPTZ`      | Optional                                         |
| `state`       | `initiative_state` | `active`, `achieved`, or `abandoned`; defaults to `active` |
| `deleted_at`  | `TIMESTAMPTZ`      | Soft delete; NULL while the initiative is live   |
| `created_at`  | `TIMESTAMPTZ`      | Written by the API; the burnup starts here       |
| `updated_at`  | `TIMESTAMPTZ`      | Maintained by trigger                            |

### `initiative_projects`

The projects an initiative belongs to, shaped like `action_item_projects`.

### `initiative_items`

Every span an item spent in an initiative. Leaving sets `left_at` rather than deleting the row, and rejoining inserts
a new one, so progress can be counted at any moment.

| Column           | Type          | Notes                                                    |
|------------------|---------------|----------------------------------------------------------|
| `id`             | `UUID`        | UUIDv7, primary key                                      |
| `initiative_id`  | `UUID`        | References `initiatives`; deleted with it                |
| `action_item_id` | `UUID`        | References `action_items`; deleted with it               |
| `joined_at`      | `TIMESTAMPTZ` | When the span began                                      |
| `left_at`        | `TIMESTAMPTZ` | When it ended, not before `joined_at`; NULL while current |
| `via_link_id`    | `UUID`        | The container that brought the item in; NULL for one added by hand |

The partial unique index `initiative_items_current_unique` on `(initiative_id, action_item_id)` where `left_at` is
NULL keeps an item in an initiative at most once at a time.

### `action_item_comments`

| Column           | Type          | Notes                                                        |
|------------------|---------------|--------------------------------------------------------------|
| `id`             | `UUID`        | UUIDv7, primary key                                          |
| `action_item_id` | `UUID`        | References `action_items`; deleted with it                   |
| `author`         | `TEXT`        | An actor: `user`, `elysia`, `session:<n>`, `project_manager:<project id>`, or `watcher:<provider>` |
| `body`           | `TEXT`        | 1 to 20,000 characters                                       |
| `created_at`     | `TIMESTAMPTZ` | Written by the API                                           |
| `updated_at`     | `TIMESTAMPTZ` | Maintained by trigger                                        |

### `action_item_events`

The history of items and initiatives, one row per change; see `docs/action-items.md`.

| Column           | Type          | Notes                                                        |
|------------------|---------------|--------------------------------------------------------------|
| `id`             | `UUID`        | UUIDv7, primary key; orders entries as they were written     |
| `action_item_id` | `UUID`        | The item it is about, if any; deleted with it                |
| `initiative_id`  | `UUID`        | The initiative it is about, if any; deleted with it          |
| `kind`           | `TEXT`        | Lowercase with underscores, such as `state_changed`          |
| `actor`          | `TEXT`        | `user:<id>`, `elysia`, `session:<n>`, `project_manager:<project id>`, `watcher:<provider>`, or `system`; `user` before accounts |
| `data`           | `JSONB`       | A JSON object shaped by `kind`                               |
| `created_at`     | `TIMESTAMPTZ` | Written by the API, the moment of the change                 |
| `changeset_id`   | `UUID`        | The changeset whose applying or undoing made the change; `ON DELETE SET NULL` |

At least one of `action_item_id` and `initiative_id` is set; both are for an item joining or leaving an initiative.
The partial index `action_item_events_changeset_id_idx` finds a changeset's entries.
`kind` and `actor` are text with shape checks rather than enums, because later stages add kinds, and Postgres refuses
a new enum value in the transaction that added it.

### `action_item_links`

What an item links to: a Jira issue, a GitHub issue or pull request, or a YouTrack issue; see `docs/action-items.md`.

| Column                 | Type                     | Notes                                                        |
|------------------------|--------------------------|--------------------------------------------------------------|
| `id`                   | `UUID`                   | UUIDv7, primary key                                          |
| `action_item_id`       | `UUID`                   | References `action_items`; deleted with it                   |
| `provider`             | `link_provider`          | `jira`, `github`, or `youtrack`                              |
| `kind`                 | `link_kind`              | `issue` or `pull_request`; only GitHub links pull requests   |
| `jira_credential_id`   | `UUID`                   | Set exactly for `jira`; deleted with the credential          |
| `github_credential_id` | `UUID`                   | Set exactly for `github`; deleted with the credential        |
| `token_account_id`     | `UUID`                   | Set exactly for `youtrack`; deleted with the account         |
| `external_id`          | `TEXT`                   | What the thing is, for good: Jira's issue id, `owner/name#12`, or YouTrack's `2-41` |
| `external_key`         | `TEXT`                   | What a person reads and a call names: `ELY-12`, `owner/name#12`, or `JL-12` |
| `url`                  | `TEXT`                   | `https://`, up to 2048 characters                            |
| `title`                | `TEXT`                   | As last read, up to 1000 characters                          |
| `is_primary`           | `BOOLEAN`                | At most one per item, by the partial unique index `action_item_links_primary_unique` |
| `observed_state`       | `link_state`             | `open`, `done`, `not_planned`, `merged`, or `closed_unmerged`, as last read |
| `observed_owner_kind`  | `action_item_owner_kind` | Whose it was when last read, in the item's terms             |
| `observed_owner_name`  | `TEXT`                   | Set exactly when `observed_owner_kind` is `other`            |
| `created_at`           | `TIMESTAMPTZ`            | Written by the API                                           |
| `updated_at`           | `TIMESTAMPTZ`            | Maintained by trigger                                        |

`action_item_links_jira_unique`, `action_item_links_github_unique`, and `action_item_links_youtrack_unique` make one
external thing, through one credential,
one link, so it is one item; they also serve the watcher's reads by credential. `action_item_links_action_item_id_idx`
serves an item's links, read on nearly every item write.

Owed writes are tried untried first, then by `last_attempt_at`, so writes that keep failing or waiting take turns
behind new ones rather than holding the front of the queue.

### `action_item_link_writes`

Provider writes an item owes and that have not landed: a close when it resolves, a comment for its primary link.

| Column            | Type              | Notes                                                        |
|-------------------|-------------------|--------------------------------------------------------------|
| `id`              | `UUID`            | UUIDv7, primary key; orders writes the way they were owed    |
| `link_id`         | `UUID`            | References `action_item_links`; deleted with it              |
| `kind`            | `link_write_kind` | `close` or `comment`; a link owes one close at most          |
| `comment_id`      | `UUID`            | Set exactly for `comment`; deleting the comment drops the write |
| `attempts`        | `INTEGER`         | How many times it was tried                                  |
| `last_error`      | `TEXT`            | The provider's last answer, or why it waits; up to 2000 characters |
| `last_attempt_at` | `TIMESTAMPTZ`     | When it was last tried                                       |
| `created_at`      | `TIMESTAMPTZ`     | Written by the API, in the transaction that owed it          |

### `action_item_issue_filings`

Issues an item is owed and that the watcher has not filed yet: an item a changeset's `file-issue` asked to have its own
issue, or a person's accepting a finding did; see `docs/action-items.md` and `docs/engine.md`, Findings.

| Column                   | Type            | Notes                                                        |
|--------------------------|-----------------|--------------------------------------------------------------|
| `id`                     | `UUID`          | UUIDv7, primary key; orders filings the way they were owed   |
| `action_item_id`         | `UUID`          | References `action_items`, unique: an item is filed once at a time; deleted with it |
| `provider`               | `link_provider` | Where it files                                               |
| `jira_credential_id`, `github_credential_id`, `token_account_id` | `UUID` | Exactly the provider's one set; deleted with the credential |
| `destination`            | `TEXT`          | A Jira project key, a GitHub `owner/name`, or a YouTrack project's short name |
| `filed_by`               | `TEXT`          | Who owes it: the proposer in its stored form, or `user:<id>` for a person who accepted a finding; the link it lands as is theirs |
| `changeset_operation_id` | `UUID`          | The operation that owed it, `ON DELETE SET NULL`             |
| `timeline_record_id`     | `UUID`          | The write's timeline record, which links to the issue once filed; `ON DELETE SET NULL` |
| `attempts`               | `INTEGER`       | How many attempts began                                      |
| `first_attempt_at`       | `TIMESTAMPTZ`   | When the first began; a later one looks for the issue it may have opened |
| `last_error`             | `TEXT`          | The provider's last answer; up to 2000 characters            |
| `last_attempt_at`        | `TIMESTAMPTZ`   | When it was last tried                                       |
| `created_at`             | `TIMESTAMPTZ`   | Written by the API, in the transaction that owed it          |

### `initiative_links`

The containers an initiative links to; see `docs/action-items.md`.

| Column                 | Type             | Notes                                                        |
|------------------------|------------------|--------------------------------------------------------------|
| `id`                   | `UUID`           | UUIDv7, primary key                                          |
| `initiative_id`        | `UUID`           | References `initiatives`; deleted with it                    |
| `provider`             | `link_provider`  | `jira` or `github`                                           |
| `kind`                 | `container_kind` | `epic` or `filter` for Jira, `milestone` or `label` for GitHub |
| `jira_credential_id`, `github_credential_id` | `UUID` | Exactly the provider's one set; deleted with the credential |
| `external_id`          | `TEXT`           | An epic's issue id, a filter's id, `owner/name#3`, or `owner/name:label` |
| `external_key`         | `TEXT`           | `ELY-7` for an epic; otherwise the same as `external_id`     |
| `url`, `title`         | `TEXT`           | As last read                                                 |
| `synced_at`            | `TIMESTAMPTZ`    | When the watcher last read every child                       |
| `sync_error`           | `TEXT`           | Why the latest read failed; NULL when it succeeded           |
| `truncated`            | `BOOLEAN`        | It held more children than the watcher reads                 |
| `created_at`           | `TIMESTAMPTZ`    | Written by the API                                           |
| `updated_at`           | `TIMESTAMPTZ`    | Maintained by trigger                                        |

A container is linked to an initiative once. `initiative_items.via_link_id` references it, `ON DELETE SET NULL`, and
names the container that brought an item in.

### `link_watch_cursors`

How far the watcher has read each credential: everything updated before `watched_through` has been applied. One row
per credential, `jira_credential_id`, `github_credential_id`, or `token_account_id` (each unique, deleted with its
credential), with
`last_error` holding why the latest pass over it failed.

### `assigned_issues`

Every issue an issue connector brought in because it is assigned to its person, and the item it raised or found
already linked; see `docs/action-items.md`, Assigned issues. One row per connector and issue is what keeps an issue from
raising a second item, whatever happened to the first since.

| Column           | Type          | Notes                                                        |
|------------------|---------------|--------------------------------------------------------------|
| `id`             | `UUID`        | UUIDv7, primary key                                          |
| `connector_id`   | `UUID`        | References `connectors`, cascading: what a removed connector brought in is forgotten |
| `external_id`    | `TEXT`        | The issue as its provider names it for good, as `action_item_links.external_id`; unique with `connector_id` |
| `action_item_id` | `UUID`        | References `action_items`, cascading; indexed                |
| `created_by`     | `UUID`        | References `users`: the system user, as the watcher          |
| `created_at`     | `TIMESTAMPTZ` | Written by the API, in the transaction that made the item    |

### `jira_done_transitions`

The done transition chosen for a Jira project, stored as the `done` status it leads into. The primary key is
`(jira_credential_id, project_key)`, deleted with the credential; `status_id` and `status_name` are what Jira reported
when it was chosen. A project with exactly one `done` status has no row.

### `changesets`

Changes anyone but the user proposes, waiting for the user's review; see `docs/action-items.md`.

| Column       | Type              | Notes                                                              |
|--------------|-------------------|--------------------------------------------------------------------|
| `id`         | `UUID`            | UUIDv7, primary key; orders changesets as they were proposed       |
| `proposer`   | `TEXT`            | `elysia`, `session:<n>`, or `project_manager:<project id>`, in the actor form history records |
| `project_id` | `UUID`            | The project a coding session proposed it in; `ON DELETE SET NULL`  |
| `summary`    | `TEXT`            | 1 to 500 characters                                                |
| `state`      | `changeset_state` | `pending`, `applied`, `rejected`, or `undone`                      |
| `decided_at` | `TIMESTAMPTZ`     | When it was applied or rejected; set exactly once it leaves `pending` |
| `undone_at`  | `TIMESTAMPTZ`     | Set exactly while `state` is `undone`                              |
| `created_at` | `TIMESTAMPTZ`     | Written by the API                                                 |
| `updated_at` | `TIMESTAMPTZ`     | Maintained by trigger                                              |

`changesets_state_idx` on `(state, id)` serves the list of pending changesets, newest first.

### `changeset_operations`

A changeset's operations, in order. `(changeset_id, position)` is unique, and the rows are deleted with the changeset.

| Column         | Type                 | Notes                                                           |
|----------------|----------------------|-----------------------------------------------------------------|
| `id`           | `UUID`               | UUIDv7, primary key                                             |
| `changeset_id` | `UUID`               | References `changesets`                                         |
| `position`     | `INTEGER`            | From 1; the order operations apply in, and how a later one names an earlier one |
| `operation`    | `JSONB`              | The change as validated: an object whose `kind` says what it does |
| `reason`       | `TEXT`               | 1 to 2000 characters                                            |
| `quote`        | `TEXT`               | What it came from, 1 to 2000 characters; optional               |
| `source`       | `TEXT`               | Where that is, 1 to 500 characters; optional                    |
| `cause_record_id` | `UUID`            | The timeline record that caused it, `ON DELETE SET NULL`; every Project Manager's operation names one, a task, or both |
| `meeting_task_id` | `UUID`            | References `meeting_tasks`, `ON DELETE SET NULL`: the meeting's task that caused a Project Manager's operation; a create naming one makes the task's item. Indexed while set |
| `decision`     | `changeset_decision` | `pending`, `approved`, or `rejected`                            |
| `outcome`      | `changeset_outcome`  | `pending`, `applied`, `failed`, `skipped`, or `undone`; only an approved operation is applied, fails, or is undone |
| `error`        | `TEXT`               | Why it failed or was skipped, up to 2000 characters             |
| `result`       | `JSONB`              | What applying it did: the ids it acted on or created, and the values it replaced |
| `undo`         | `JSONB`              | What undoing it did and could not do; NULL until it is undone   |

Operations are JSON rather than a table per kind, so a kind is added without a migration; the API validates each one
before it is stored.

### `users`

Every principal that can create something: people, who sign in through Kratos, and machines seeded with fixed ids.
See `docs/auth.md`.

| Column               | Type          | Notes                                                                 |
|----------------------|---------------|-----------------------------------------------------------------------|
| `id`                 | `UUID`        | UUIDv7 for people; fixed for machines                                 |
| `kratos_identity_id` | `UUID`        | Unique; set exactly for people                                        |
| `email`              | `TEXT`        | Unique, lowercase, 3 to 320 characters; set exactly for people        |
| `name`               | `TEXT`        | 1 to 100 characters, copied from Kratos for people                    |
| `role`               | `user_role`   | `admin`, `member`, `guest` for people; `agent`, `system` for machines; NULL while pending |
| `status`             | `user_status` | `pending`, `active`, `disabled`; set exactly for people               |
| `approved_at`        | `TIMESTAMPTZ` | When approved; the first person is approved as they sign up           |
| `last_seen_at`       | `TIMESTAMPTZ` | Their last request, to within five minutes                            |
| `created_at`         | `TIMESTAMPTZ` | Set on insert                                                         |
| `updated_at`         | `TIMESTAMPTZ` | Maintained by trigger                                                 |

`users_person_or_machine` makes a row exactly one of the two, so a machine never holds a Kratos identity and a person
never holds a machine's role. `users_pending_has_no_role` ties a missing role to being pending. The machines are
Elysium itself (`…0001`, `system`), Elysia (`…0002`), every coding session's agent (`…0003`), and every project's
Project Manager (`…0004`), each an `agent` but the first.

### `authenticator_nicknames`

What a person calls their authenticator app, such as "Bitwarden". A child of `users`: Kratos holds the app, one per
identity, and this only names it. See `docs/auth.md`, Authenticator nicknames.

| Column       | Type          | Notes                                                              |
|--------------|---------------|--------------------------------------------------------------------|
| `user_id`    | `UUID`        | Primary key; references `users`, `ON DELETE CASCADE`               |
| `nickname`   | `TEXT`        | 1 to 50 characters, already trimmed (`authenticator_nicknames_length`) |
| `created_at` | `TIMESTAMPTZ` | Set on insert                                                      |
| `updated_at` | `TIMESTAMPTZ` | Maintained by trigger                                              |

### `authenticator_replacements`

The new authenticator app a person is moving to, held until a code from it is verified, so the app in use keeps
working until then. A child of `users`, one row at a time; starting again replaces it. See `docs/auth.md`,
Authenticator apps.

| Column             | Type          | Notes                                                                   |
|--------------------|---------------|-------------------------------------------------------------------------|
| `user_id`          | `UUID`        | Primary key; references `users`, `ON DELETE CASCADE`                    |
| `secret_encrypted` | `BYTEA`       | The new app's TOTP secret, sealed (`docs/secrets.md`)                   |
| `expires_at`       | `TIMESTAMPTZ` | 15 minutes after the start; refused after it, removed at the next start |
| `created_at`       | `TIMESTAMPTZ` | Set on insert, and again when replaced                                  |

### `workspace_settings`

One row, always present (`singleton` is its key and must be true).

| Column        | Type          | Notes                                                        |
|---------------|---------------|--------------------------------------------------------------|
| `signup_open` | `BOOLEAN`     | Defaults to true; the first person may sign up regardless    |
| `require_mfa` | `BOOLEAN`     | Defaults to false                                            |
| `agent_configuration_repository` | `TEXT` | Nullable; the https repository agents' `~/.claude` comes from, no credentials, at most 2048 characters |
| `agent_configuration_github_credential_id` | `UUID` | Nullable; the GitHub token that reads it, `NULL` to read it without one. References `github_credentials`, `SET NULL`; only for a github.com repository |
| `backup_storage_location_id` | `UUID` | Nullable; the storage location the databases are backed up to, `NULL` for none. References `storage_locations` through `workspace_settings_backup_storage_location`, so a chosen location cannot be deleted |
| `page_attachment_storage_location_id` | `UUID` | Nullable; the storage location new page and meeting attachments go to, `NULL` for none. References `storage_locations` through `workspace_settings_page_attachment_storage_location`, so a chosen location cannot be deleted |
| `meeting_attachment_max_bytes` | `BIGINT` | The largest file a meeting takes, 1 GB by default, from 1 to 5 GiB (`workspace_settings_meeting_attachment_max_bytes`) |
| `worker_pool_size` | `INTEGER` | Nullable; how many workers run at once, at least one with no maximum, `NULL` for the default (one per CPU core the engine sees minus two, at least one) |
| `updated_by`  | `UUID`        | References `users`                                           |
| `updated_at`  | `TIMESTAMPTZ` | Maintained by trigger                                        |

### `user_events`

What happened to accounts and workspace settings, one row per change.

| Column       | Type          | Notes                                                                   |
|--------------|---------------|-------------------------------------------------------------------------|
| `id`         | `UUID`        | UUIDv7, primary key                                                     |
| `user_id`    | `UUID`        | Whose account; NULL for a workspace setting; `ON DELETE SET NULL`       |
| `actor_id`   | `UUID`        | Who made the change; `ON DELETE SET NULL`, for a rejected sign-up's own events |
| `kind`       | `TEXT`        | Such as `signed_up`, `approved`, `rejected`, `role_changed`, `disabled`, and for workspace settings `workspace_settings_changed`, `learning_category_added`, `_changed`, or `_removed`, `model_setting_changed` (`{ agent, before, after }`), and `kind_model_setting_changed` (`{ kind, projectId, before, after }`, `projectId` `null` for the workspace's); `before` and `after` are `null` for what it followed |
| `data`       | `JSONB`       | A JSON object shaped by `kind`, with values before and after            |
| `created_at` | `TIMESTAMPTZ` | Set on insert                                                           |

### `elysia_state`

Elysia's settings and her session. One row, always present (`singleton` is its key and must be true). See
`docs/elysia.md`.

| Column               | Type          | Notes                                                                    |
|----------------------|---------------|--------------------------------------------------------------------------|
| `paused`             | `BOOLEAN`     | Defaults to false; while true, no turn starts                            |
| `availability`       | `JSONB`       | An array of `{ startMinute, endMinute }` UTC minute-of-week windows; empty means any time |
| `usage_paused_until` | `TIMESTAMPTZ` | While in the future, no turn starts                                      |
| `runner_session_id`  | `TEXT`        | The Claude Code session her next turn resumes                            |
| `updated_by`         | `UUID`        | References `users`                                                       |
| `updated_at`         | `TIMESTAMPTZ` | Maintained by trigger                                                    |

### `elysia_messages`

The conversation between people and Elysia. A message is hers when `created_by` is her seeded user. Never edited.

| Column       | Type          | Notes                                         |
|--------------|---------------|-----------------------------------------------|
| `id`         | `UUID`        | UUIDv7, primary key, so ids order messages    |
| `body`       | `TEXT`        | 1 to 20,000 characters                        |
| `created_by` | `UUID`        | References `users`                            |
| `created_at` | `TIMESTAMPTZ` | Set on insert                                 |

### `elysia_turns`

One run of her session and what started it.

| Column           | Type                 | Notes                                                                 |
|------------------|----------------------|-----------------------------------------------------------------------|
| `id`             | `UUID`               | UUIDv7, primary key                                                   |
| `trigger_kind`   | `TEXT`               | `message`, `answer`, `event`, or `idle`; lowercase, so later kinds need no migration |
| `trigger_detail` | `JSONB`              | A JSON object: `{ messageId }`, `{ questionId }`, `{ events, omitted }`, or `{ workIds }` |
| `status`         | `elysia_turn_status` | `queued`, `running`, `succeeded`, `failed`, `cancelled`              |
| `summary`        | `TEXT`               | Up to 1,000 characters on what the turn did                           |
| `error`          | `TEXT`               | Up to 2,000 characters; set exactly when `failed`                     |
| `created_by`     | `UUID`               | The person who wrote or answered; the system user for events          |
| `created_at`     | `TIMESTAMPTZ`        | Set on insert                                                         |
| `started_at`     | `TIMESTAMPTZ`        | Set exactly once it ran                                               |
| `finished_at`    | `TIMESTAMPTZ`        | Set exactly once it succeeded or failed                               |

A partial index on queued and running turns serves the queue.

### `elysia_questions`

What Elysia asks a person when she is blocked.

| Column           | Type                     | Notes                                                       |
|------------------|--------------------------|-------------------------------------------------------------|
| `id`             | `UUID`                   | UUIDv7, primary key                                         |
| `prompt`         | `TEXT`                   | 1 to 2,000 characters                                       |
| `options`        | `JSONB`                  | Up to four suggested answers, as strings                    |
| `status`         | `elysia_question_status` | `open`, `answered`, `dismissed`                             |
| `answer`         | `TEXT`                   | 1 to 2,000 characters; set exactly when `answered`          |
| `action_item_id` | `UUID`                   | The item it is about; `ON DELETE SET NULL`                  |
| `created_by`     | `UUID`                   | References `users`                                          |
| `created_at`     | `TIMESTAMPTZ`            | Set on insert                                               |
| `answered_by`    | `UUID`                   | Who answered or dismissed it; set exactly once it is closed |
| `answered_at`    | `TIMESTAMPTZ`            | Set exactly once it is closed                               |

### `elysia_work`

What Elysia has taken on.

| Column           | Type                 | Notes                                                           |
|------------------|----------------------|-----------------------------------------------------------------|
| `id`             | `UUID`               | UUIDv7, primary key                                             |
| `title`          | `TEXT`               | 1 to 200 characters                                             |
| `detail`         | `TEXT`               | Up to 2,000 characters, where it stands in her words            |
| `status`         | `elysia_work_status` | `working`, `ready`, `blocked`, `gave-up`, `done`                |
| `ready_at`       | `TIMESTAMPTZ`        | Her estimate of when it is ready for review                     |
| `action_item_id` | `UUID`               | The item it serves; `ON DELETE SET NULL`                        |
| `revisits`       | `INTEGER`            | How often she came back to it                                   |
| `revisited_at`   | `TIMESTAMPTZ`        | When she last came back to it                                   |
| `created_by`     | `UUID`               | References `users`                                              |
| `created_at`     | `TIMESTAMPTZ`        | Set on insert                                                   |
| `updated_at`     | `TIMESTAMPTZ`        | Maintained by trigger                                           |
### `project_managers`

A project's Project Manager, once something woke it; see `docs/engine.md`, Project Managers. Its first turn creates
it.

| Column               | Type          | Notes                                                                  |
|----------------------|---------------|------------------------------------------------------------------------|
| `project_id`         | `UUID`        | Primary key; references `projects`, cascading                          |
| `runner_session_id`  | `TEXT`        | The Claude Code session its next turn resumes; 1 to 200 characters     |
| `usage_paused_until` | `TIMESTAMPTZ` | No turn starts before this, after every credential reported a usage limit |
| `created_by`         | `UUID`        | References `users`: the Project Managers' machine user                 |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger                |

### `project_manager_turns`

One turn of a project's Project Manager. Created as it starts, so there are no queued turns: what waits is its wakes.
`project_manager_turns_one_running`, unique on `project_id` while `running`, refuses a project a second running turn.

| Column               | Type                          | Notes                                                    |
|----------------------|-------------------------------|----------------------------------------------------------|
| `id`                 | `UUID`                        | UUIDv7, primary key                                      |
| `project_id`         | `UUID`                        | References `project_managers`, cascading                 |
| `status`             | `project_manager_turn_status` | `running`, `succeeded`, or `failed`                      |
| `summary`            | `TEXT`                        | Its `Summary:` line, 1 to 1,000 characters               |
| `reply`              | `TEXT`                        | What it answered, before its summary, up to 20,000 characters |
| `error`              | `TEXT`                        | Set exactly while `failed`, up to 2,000 characters       |
| `log`                | `TEXT`                        | What it said and which tools it called, one line each, the newest 64 KiB |
| `timeline_record_id` | `UUID`                        | The record it put on its project's timeline; `SET NULL`  |
| `created_by`         | `UUID`                        | References `users`: the Project Managers' machine user   |
| `started_at`         | `TIMESTAMPTZ`                 | Set on insert                                            |
| `finished_at`        | `TIMESTAMPTZ`                 | Set exactly once it is not `running`                     |

### `project_manager_wakes`

What wakes a project's Project Manager, written in the transaction that caused it. A wake waits while `turn_id` is
NULL; a turn takes every waiting wake of its project as it starts. Indexed for what waits per project, and by turn.
`project_manager_wakes_record_waits_once` keeps one waiting wake per record and project,
`project_manager_wakes_promise_waits_once` one per promise and project,
`project_manager_wakes_assignment_waits_once` one per assigned item and project, and
`project_manager_wakes_task_waits_once` one per meeting task and project. Every publish of a brief that wakes its
Project Manager is a wake of its own, so a turn names each edit. `project_manager_wakes_by_action_item` finds an item's
newest assignment, for where its plan stands.

| Column               | Type                        | Notes                                                       |
|----------------------|-----------------------------|-------------------------------------------------------------|
| `id`                 | `UUID`                      | UUIDv7, primary key                                         |
| `project_id`         | `UUID`                      | References `projects`, cascading                            |
| `kind`               | `project_manager_wake_kind` | `record`, `batch`, `message`, `promise`, `brief`, `assignment`, or `task` |
| `timeline_record_id` | `UUID`                      | Set exactly for `record`: the record attributed to the project; cascading |
| `work_batch_id`      | `UUID`                      | Set exactly for `batch`: the batch that finished; cascading |
| `body`               | `TEXT`                      | Set exactly for `message`: what a person or Elysia wrote, 1 to 20,000 characters |
| `turn_id`            | `UUID`                      | The turn that took it; NULL while it waits; cascading       |
| `created_by`         | `UUID`                      | References `users`: who wrote, who attributed the record, or the system user for a result |
| `created_at`         | `TIMESTAMPTZ`               | Set on insert                                               |
| `promise_id`         | `UUID`                      | Set exactly for `promise`: the promise found, or whose work finished; cascading |
| `page_version`       | `INTEGER`                   | Set exactly for `brief`: the version of the project's brief someone else published |
| `action_item_id`     | `UUID`                      | Set exactly for `assignment`: the item a person assigned it; cascading |
| `meeting_task_id`    | `UUID`                      | Set exactly for `task`: the meeting's task for its project, new or changed; references `meeting_tasks`, cascading |

### `promises`

What a person promised to tell someone once work is done, found by segmenting a conversation, kept by its project's
Project Manager; see `docs/engine.md`, Follow-ups. Indexed by project, and by each of its batch, work item, and
follow-up item while set.

| Column               | Type            | Notes                                                                 |
|----------------------|-----------------|-----------------------------------------------------------------------|
| `id`                 | `UUID`          | UUIDv7, primary key                                                   |
| `project_id`         | `UUID`          | References `projects`, cascading: whose Project Manager keeps it      |
| `timeline_record_id` | `UUID`          | References `timeline_records`, cascading, unique: the person's message it was made in, which its follow-up answers |
| `recipient`          | `TEXT`          | Who it is for, by name; 1 to 200 characters                           |
| `summary`            | `TEXT`          | What was promised, as a short phrase; 1 to 500 characters             |
| `state`              | `promise_state` | `waiting`, `due`, `drafted`, `kept`, or `dismissed`                   |
| `work_batch_id`      | `UUID`          | References `work_batches`, `SET NULL`: the batch that keeps it        |
| `work_item_id`       | `UUID`          | References `action_items`, `SET NULL`: the item that keeps it; at most one of the two is set (`promises_one_work`) |
| `due_at`             | `TIMESTAMPTZ`   | When its work finished; set exactly once it is past `waiting`         |
| `follow_up_item_id`  | `UUID`          | References `action_items`, `SET NULL`: the item its follow-up is, once drafted |
| `created_by`         | `UUID`          | References `users`: Elysia, who found it                              |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger                 |

### `work_batches`

Work a Project Manager submitted for the worker pool; see `docs/engine.md`, Work batches and Workers and the pool.
`work_batches_queue` on `(priority, scheduled_for, created_at)` while `queued` reads what waits, which the scheduler
orders in code; `work_batches_in_slots` on `project_id` while claimed counts each project's slots; and
`work_batches_proposed` on `(project_id, id)` while `proposed` reads what waits for a person and counts each project's.
`work_batches_claimed_while_in_a_slot` holds a claim exactly on a batch `assigned` or `running`, so nothing writes a
claim on a proposed one. A batch stored before origins existed has the origin its provenance proves, by the rules the
API applies (`docs/engine.md`, Origins and generations), generation 0, and no parents.

| Column               | Type                  | Notes                                                             |
|----------------------|-----------------------|-------------------------------------------------------------------|
| `id`                 | `UUID`                | UUIDv7, primary key                                               |
| `project_id`         | `UUID`                | References `projects`, cascading                                  |
| `title`              | `TEXT`                | 1 to 200 characters                                               |
| `goal`               | `TEXT`                | What the worker should accomplish, 1 to 20,000 characters         |
| `serves`             | `TEXT`                | 1 to 300 characters: the goal in its project's brief it serves, as the brief's heading spelled it at submission; NULL for a batch from before briefs |
| `repositories`       | `JSONB`               | `[{ url, baseBranch? }]`, at most 16                              |
| `priority`           | `work_batch_priority` | `asap`, `normal`, or `scheduled`                                  |
| `scheduled_for`      | `TIMESTAMPTZ`         | Set exactly for `scheduled`                                       |
| `state`              | `work_batch_state`    | `proposed` (the default), `queued`, `paused`, `assigned`, `running`, `succeeded`, `failed`, `cancelled`, or `rejected`; `work_batch::submit` names it, `queued` only for a human-rooted batch its project's level lets through |
| `result`             | `TEXT`                | What its worker reported, or why it was cancelled or rejected     |
| `turn_id`            | `UUID`                | The turn that submitted it; `SET NULL`                            |
| `source_record_ids`  | `JSONB`               | The timeline records that called for it, as an array of ids       |
| `action_item_ids`    | `JSONB`               | The action items it works on, as an array of ids                  |
| `source_message_ids` | `JSONB`               | The messages to its Project Manager that called for it, as an array of `project_manager_wakes` ids |
| `origin`             | `work_batch_origin`   | `human_rooted` or `found`, worked out from its provenance as it is submitted; no default |
| `generation`         | `INTEGER`             | How many batches led to it, at least 0; no default. The limit is a code constant (`work_batch::lineage::MAX_GENERATION`) |
| `parent_batch_ids`   | `JSONB`               | The batches it was planned from, as an array of ids               |
| `timeline_record_id` | `UUID`                | Its record on the project's timeline; `SET NULL`                  |
| `created_by`         | `UUID`                | References `users`: the Project Managers' machine user, or the person who accepted the finding it was made from |
| `created_at`, `updated_at` | `TIMESTAMPTZ`   | Set on insert; `updated_at` maintained by trigger                 |
| `started_at`         | `TIMESTAMPTZ`         | Set exactly once a worker started on it, so never for a waiting, cancelled, or rejected batch |
| `finished_at`        | `TIMESTAMPTZ`         | Set exactly once it ended, was cancelled, or was rejected         |
| `pinned_at`          | `TIMESTAMPTZ`         | Set while someone asked for it to run next; only while `queued`   |
| `assigned_at`        | `TIMESTAMPTZ`         | When the pool gave it a slot; set while `assigned` or `running`, kept once it ended, never while it waits or once rejected |
| `claim_token`        | `UUID`                | The engine holding its slot; set exactly while `assigned` or `running` |
| `lease_expires_at`   | `TIMESTAMPTZ`         | Until when that engine holds the slot unless it renews; set exactly with `claim_token` |
| `worker_session_id`  | `TEXT`                | The Claude Code session its worker's turns resume, once its first turn reported one |
| `waiting_on`         | `work_batch_wait`     | `review`, `ci`, or `question`: what its worker waits on, keeping its container; only while `running` |
| `waiting_detail`     | `TEXT`                | 1 to 4,000 characters: what it waits for, in its worker's words; set exactly with `waiting_on` |
| `waiting_until`      | `TIMESTAMPTZ`         | When its worker looks again, or the moment a question was answered; set exactly with `waiting_on` |
| `worker_report`      | `JSONB`               | How its worker said its last turn ended, filed by the worker's tools until the engine takes it: `{ kind: "finished", succeeded, result }` or `{ kind: "waiting", on, detail, until }` |
| `worker_log`         | `TEXT`                | What its worker said and which tools it called, one line each, scrubbed, the newest 64 KiB; empty by default |
| `configuration`      | `JSONB`               | The configuration its worker runs on, recorded as its container fetched it: `{ repository, commit, subject, fetchedAt, skipped }`; cleared when it goes back to the queue |

### `work_batch_events`

A batch's history, one row per change, shaped like `workflow_run_events`; see `docs/engine.md`, Work batches.

| Column          | Type          | Notes                                                                  |
|-----------------|---------------|------------------------------------------------------------------------|
| `id`            | `UUID`        | UUIDv7, primary key, so entries order as written                       |
| `work_batch_id` | `UUID`        | References `work_batches`, cascading                                   |
| `kind`          | `TEXT`        | `submitted`, `held_for_approval`, `edited`, `approved`, `rejected`, `pinned`, `unpinned`, `reprioritized`, `paused`, `resumed`, `cancelled`, `assigned`, `unassigned`, `started`, `requeued`, `waiting`, `answered`, or `finished` |
| `data`          | `JSONB`       | A JSON object shaped by `kind`; `submitted` holds `{ autonomy }` when the project's level let the batch queue without an approval |
| `created_by`    | `UUID`        | Who did it: a person, Elysia, the Project Managers' machine user, the system user for the pool, or the coding agent for its worker |
| `created_at`    | `TIMESTAMPTZ` | When it was done                                                       |

### `findings`

Work a worker or a Project Manager noticed that nobody asked for, waiting for a person to accept or dismiss; see
`docs/engine.md`, Findings. `findings_once` on `(project_id, fingerprint)` makes a second report of the same finding land
on the first, and `findings_open` on `(project_id, id)` while `open` reads a project's list and every project's count.
Rows are never deleted, so a dismissed finding stays dismissed.

| Column                    | Type            | Notes                                                              |
|---------------------------|-----------------|--------------------------------------------------------------------|
| `id`                      | `UUID`          | UUIDv7, primary key, so ids order findings as first reported       |
| `project_id`              | `UUID`          | References `projects`, cascading                                   |
| `fingerprint`             | `TEXT`          | 64 hex digits: the SHA-256 of its repository, place, and title, each normalized by the API |
| `title`                   | `TEXT`          | 1 to 200 characters                                                |
| `detail`                  | `TEXT`          | What was seen, why it matters, and what acting on it involves; 1 to 10,000 characters |
| `repository_url`          | `TEXT`          | The repository it was seen in, held to a coding session's rules; up to 2,048 characters |
| `location`                | `TEXT`          | Where: a path, a URL, or a place's name; 1 to 500 characters       |
| `cost`                    | `finding_cost`  | `small` (an hour or less), `medium` (about a batch), or `large` (more than one) |
| `state`                   | `finding_state` | `open` (the default), `accepted`, or `dismissed`                   |
| `work_batch_id`           | `UUID`          | The batch whose worker reported it first; `SET NULL`               |
| `project_manager_turn_id` | `UUID`          | The Project Manager's turn that reported it first; `SET NULL`. At most one of the two |
| `reports`                 | `INTEGER`       | How many times it was reported, the first included; at least 1     |
| `last_reported_at`        | `TIMESTAMPTZ`   | When it was last reported                                          |
| `decided_by`, `decided_at` | `UUID`, `TIMESTAMPTZ` | The person who accepted or dismissed it, and when; set exactly once it is not `open` |
| `dismissal_reason`        | `TEXT`          | Why it was dismissed, when the person said; 1 to 2,000 characters, only when `dismissed` |
| `action_item_id`          | `UUID`          | The item accepting it made; `SET NULL`, only when `accepted`       |
| `accepted_batch_id`       | `UUID`          | The batch accepting it proposed; `SET NULL`, only when `accepted`  |
| `created_by`              | `UUID`          | References `users`: the coding agent for a worker, the Project Managers' machine user for a Project Manager |
| `created_at`              | `TIMESTAMPTZ`   | When it was first reported                                         |

### `work_batch_approval_items`

The one inbox item per project that asks a person to approve its proposed batches; see `docs/engine.md`, Proposed
batches. A project has one row at most, naming the item it raised last; once its person resolves or dismisses that
item, the next proposal raises a new one and the row names it instead.

| Column           | Type          | Notes                                                                   |
|------------------|---------------|-------------------------------------------------------------------------|
| `project_id`     | `UUID`        | Primary key; references `projects`, cascading                           |
| `action_item_id` | `UUID`        | References `action_items`, cascading: the item that counts its proposals |
| `created_by`     | `UUID`        | References `users`: the Project Managers' machine user                  |
| `created_at`, `updated_at` | `TIMESTAMPTZ` | Set on insert; `updated_at` maintained by trigger             |

### `work_batch_pull_requests`

The pull requests a batch's worker recorded, and where each stands; see `docs/engine.md`, Merging. A pull request is
one batch's: `work_batch_pull_requests_once` is unique on `(lower(repository), number)`, since GitHub names
repositories ignoring case. `work_batch_pull_requests_of_batch` on `work_batch_id` reads a batch's. Only an `open` row
moves to `merged` (`pull_request::mark_merged`), in the transaction that puts the merge on the timeline, so a merge is
recorded once. `work_batch_pull_requests_merged_whole` holds `merged_at` and `merged_by` set exactly while `merged`,
no `held_because` then, and `merge_method` set exactly when Elysium merged it.

| Column               | Type                        | Notes                                                    |
|----------------------|-----------------------------|----------------------------------------------------------|
| `id`                 | `UUID`                      | UUIDv7, primary key, so a batch's order as recorded      |
| `work_batch_id`      | `UUID`                      | References `work_batches`, cascading                     |
| `repository`         | `TEXT`                      | `owner/name` as GitHub spells it, 3 to 140 characters    |
| `number`             | `INTEGER`                   | Greater than zero                                        |
| `url`                | `TEXT`                      | Where a person reads it, 1 to 2,048 characters           |
| `title`              | `TEXT`                      | At most 1,000 characters, as GitHub reported it when recorded |
| `state`              | `pull_request_state`        | `open`, `merged`, or `closed` (without merging), as the last look read it |
| `merged_at`          | `TIMESTAMPTZ`               | When it merged                                           |
| `merged_by`          | `TEXT`                      | The login GitHub says merged it                          |
| `merged_by_elysium`  | `BOOLEAN`                   | Whether Elysium merged it; false by default              |
| `merge_method`       | `pull_request_merge_method` | `squash`, `merge`, or `rebase`, when Elysium merged it   |
| `held_because`       | `TEXT`                      | 1 to 4,000 characters: why the last look left it open    |
| `looked_at`          | `TIMESTAMPTZ`               | When the engine last looked at it                        |
| `timeline_record_id` | `UUID`                      | References `timeline_records`, `SET NULL`: what its merge put on the timeline |
| `created_by`         | `UUID`                      | References `users`: the coding agent                     |
| `created_at`         | `TIMESTAMPTZ`               | When its worker recorded it                              |

### `worker_pool`

What the engine's worker pool knows that the work queue shows. One row (`singleton` is its key and must be true),
written by the engine as it looks; absent until an engine first has.

| Column             | Type          | Notes                                                                 |
|--------------------|---------------|-----------------------------------------------------------------------|
| `cpu_count`        | `INTEGER`     | The CPU cores the engine sees, at least one; the default size follows it |
| `usage_held_until` | `TIMESTAMPTZ` | Set while every Claude credential that could serve is at its usage limit: no slot is handed out before it |
| `created_by`       | `UUID`        | References `users`: the system user                                   |
| `updated_at`       | `TIMESTAMPTZ` | Maintained by trigger                                                 |

### `agent_configuration_status`

How the last fetch of the configuration repository went, which decides whether workers and Project Managers may start
(`crate::agent_configuration`, `docs/coding.md`, `~/.claude`). One row (`singleton` is its key and must be true), written
by the migration and then by the engine as it fetches, and by a worker whose own fetch failed.

| Column                 | Type          | Notes                                                                 |
|------------------------|---------------|-----------------------------------------------------------------------|
| `repository`           | `TEXT`        | The repository the last attempt read; `NULL` before any. A status about another repository or token than the settings name says nothing of the current one |
| `github_credential_id` | `UUID`        | The token it was read with, as recorded, not a reference              |
| `commit_sha`           | `TEXT`        | The 40-character commit the last good fetch of that repository and token checked out |
| `commit_subject`       | `TEXT`        | Its subject; set exactly with `commit_sha`                            |
| `fetched_at`           | `TIMESTAMPTZ` | When; set exactly with `commit_sha`                                   |
| `skipped`              | `JSONB`       | An array of what that checkout left out: `[{ kind, name, reason, detail? }]` |
| `error`                | `TEXT`        | At most 4,000 characters: why the last attempt failed, while it had   |
| `failed_at`            | `TIMESTAMPTZ` | When; set exactly with `error`                                        |
| `inbox_item_id`        | `UUID`        | References `action_items`, `SET NULL`: the one item raised while held-back work waits |
| `created_by`           | `UUID`        | References `users`: the system user                                   |
| `updated_at`           | `TIMESTAMPTZ` | Maintained by trigger                                                 |

### `database_backups`

Every backup of the databases to a storage location, and how it ended; see `docs/infrastructure.md`, Backups. At most
one row is `running`, enforced by the partial unique index `database_backups_one_running`; `database_backups_started_at`
serves the schedule's and Settings' reads.

| Column                | Type                      | Notes                                                            |
|-----------------------|---------------------------|------------------------------------------------------------------|
| `id`                  | `UUID`                    | UUIDv7, primary key                                              |
| `trigger`             | `database_backup_trigger` | `schedule`, `admin`, or `deployment`                             |
| `status`              | `database_backup_status`  | `running`, `succeeded`, or `failed`                              |
| `storage_location_id` | `UUID`                    | Where the files went; `ON DELETE SET NULL`, for a location no longer chosen |
| `started_at`          | `TIMESTAMPTZ`             | To the second: it names the backup's files                      |
| `finished_at`         | `TIMESTAMPTZ`             | Set exactly once it leaves `running`                             |
| `size_bytes`          | `BIGINT`                  | Every dump together; set exactly when it succeeded               |
| `error`               | `TEXT`                    | Why it failed, 1 to 2000 characters; set exactly when it failed  |
| `created_by`          | `UUID`                    | The admin who asked for it, or the system user for the schedule and a deploy |
| `claimed_at`          | `TIMESTAMPTZ`             | When the engine took it. One the API asked for is `running` and untaken until then; one taken and still `running` when the engine starts was cut short |

### `session_turns`

Every turn a coding or Studio session was given, and the engine's queue; see `docs/coding.md`, A turn. The API queues
a turn; the engine takes each session's oldest queued turn once the session has none running, and finishes it.
`session_turns_unfinished` serves both the queue and what a session is doing.

| Column                 | Type                  | Notes                                                              |
|------------------------|-----------------------|--------------------------------------------------------------------|
| `id`                   | `UUID`                | UUIDv7, primary key; the runner's turn id, which its events carry   |
| `session_id`           | `BIGINT`              | References `coding_sessions`, cascading                            |
| `prompt`               | `TEXT`                | What the person asked                                              |
| `system_prompt_append` | `TEXT`                | Appended to the agent's system prompt, such as Studio's instructions |
| `status`               | `session_turn_status` | `queued`, `running` (the engine took it), or `finished`            |
| `stop_requested`       | `BOOLEAN`             | Someone asked it to stop: never started if queued, cancelled if running |
| `queued_at`            | `TIMESTAMPTZ`         | When it was queued                                                 |
| `started_at`           | `TIMESTAMPTZ`         | When its CLI started, once it had a slot; never while `queued`     |
| `finished_at`          | `TIMESTAMPTZ`         | Set exactly when it is `finished`                                  |
| `created_by`           | `UUID`                | References `users`; the person who gave the prompt                 |

### `agent_tool_grants`

Each running turn's access to Elysium's tool servers; see `docs/coding.md`, Elysium's tools. The engine grants, renews
each grant's lease while its turn runs, and revokes; the API checks every call, refusing a grant whose lease lapsed.
Each new grant deletes the lapsed ones. Only the SHA-256 of each bearer token is stored.

| Column         | Type          | Notes                                                                   |
|----------------|---------------|-------------------------------------------------------------------------|
| `token_sha256` | `BYTEA`       | Primary key, 32 bytes                                                   |
| `scope`        | `JSONB`       | Who calls: a session with its project, item, and directory, one of Elysia's turns, or a Project Manager's turn |
| `servers`      | `TEXT[]`      | The tool servers it may call, by name                                   |
| `created_by`   | `UUID`        | The coding agent, Elysia, or the Project Managers' machine user         |
| `created_at`   | `TIMESTAMPTZ` | When it was granted                                                     |
| `expires_at`   | `TIMESTAMPTZ` | When its lease lapses: 60 seconds past its last renewal, on Postgres's clock |

### `process_activities`

What each process runs that a restart of the stack would cut short, which auto-update waits for; see
`api/src/activity.rs`. A row counts only until `expires_at`, which its process renews every 15 seconds while it holds
anything, so a process that died holds nothing a minute later.

| Column         | Type          | Notes                                                                   |
|----------------|---------------|-------------------------------------------------------------------------|
| `process_id`   | `UUID`        | Primary key; new at every start of the process                          |
| `activities`   | `JSONB`       | `[{ "kind", "label", "startedAt" }]`, oldest first                      |
| `expires_at`   | `TIMESTAMPTZ` | The lease                                                               |
| `created_by`   | `UUID`        | The system user                                                         |

### `update_checks`

What auto-update learned, in one row, wherever it learned it: what Docker Hub listed at the engine's last check, when
the updater last asked, and a release an admin asked for; see `docs/infrastructure.md`, Auto-update.

| Column              | Type          | Notes                                                             |
|---------------------|---------------|-------------------------------------------------------------------|
| `singleton`         | `BOOLEAN`     | Primary key, always true                                          |
| `newest_release`    | `TEXT`        | The newest release at the last check that answered                |
| `last_checked_at`   | `TIMESTAMPTZ` | When Docker Hub was last asked                                    |
| `last_check_error`  | `TEXT`        | Why the last check failed, until one succeeds                     |
| `updater_seen_at`   | `TIMESTAMPTZ` | When the updater last asked what to do                            |
| `requested_release` | `TEXT`        | A release an admin asked for with Update now; cleared when its update ends |
| `requested_by`      | `UUID`        | Who asked; set exactly with `requested_release`                   |
| `updated_by`, `updated_at` | `UUID`, `TIMESTAMPTZ` | The last change                                    |

### `deployment_updates`

Every release the updater was told to apply, and how it ended; see `docs/infrastructure.md`. At most one row is
`applying`, enforced by the partial unique index `deployment_updates_one_applying`.

| Column         | Type                       | Notes                                                             |
|----------------|----------------------------|-------------------------------------------------------------------|
| `id`           | `UUID`                     | UUIDv7, primary key                                               |
| `from_version` | `TEXT`                     | The release running when it began, `X.Y.Z`                        |
| `to_version`   | `TEXT`                     | The release applied, `X.Y.Z`                                      |
| `status`       | `deployment_update_status` | `applying`, `succeeded`, or `rolled_back`                         |
| `reason`       | `TEXT`                     | Why it rolled back, 1 to 2000 characters; set exactly when it did |
| `started_at`   | `TIMESTAMPTZ`              | Set on insert                                                     |
| `finished_at`  | `TIMESTAMPTZ`              | Set exactly once it leaves `applying`                             |
| `created_by`   | `UUID`                     | The admin who asked for it, or the system user for an automatic update |

### `workflow_runs`

Every run of a workflow on a source; see `docs/engine.md`, Runs. `workflow_runs_one_live_per_source`, a partial unique
index on the workflow and source where the run has not succeeded, keeps one live run per source, which starting it
again answers.

| Column        | Type                 | Notes                                                                  |
|---------------|----------------------|------------------------------------------------------------------------|
| `id`          | `UUID`               | UUIDv7, primary key                                                    |
| `workflow`    | `TEXT`               | The workflow's name in code, `snake_case`                              |
| `source_kind` | `TEXT`               | What it is about, such as `meeting`, `snake_case`                      |
| `source_id`   | `TEXT`               | That thing's id, 1 to 255 characters                                   |
| `state`       | `workflow_run_state` | `queued`, `running`, `held`, `failed`, or `succeeded`, derived from its steps |
| `input`       | `JSONB`              | What its first step receives                                           |
| `error`       | `TEXT`               | The failed step's reason, 1 to 2000 characters; set exactly while `failed` |
| `created_by`  | `UUID`               | Who or what started it: a person, or a machine user                    |
| `created_at`  | `TIMESTAMPTZ`        | When it was started                                                    |
| `started_at`  | `TIMESTAMPTZ`        | When its first step was first claimed                                  |
| `finished_at` | `TIMESTAMPTZ`        | Set exactly while `failed` or `succeeded`                              |
| `updated_at`  | `TIMESTAMPTZ`        | Maintained by trigger                                                  |

### `step_jobs`

Each step of a run, in order, and the engine's queue; see `docs/engine.md`, Runs. `step_jobs_claimable` (queued, by
`available_at`) and `step_jobs_leased` (running, by `lease_expires_at`) serve the claim.

| Column             | Type             | Notes                                                                  |
|--------------------|------------------|------------------------------------------------------------------------|
| `id`               | `UUID`           | UUIDv7, primary key                                                    |
| `run_id`           | `UUID`           | References `workflow_runs`, cascading                                  |
| `position`         | `INTEGER`        | Orders the run's steps, unique per run; never shown                    |
| `step`             | `TEXT`           | The step's name in code, `snake_case`                                  |
| `state`            | `step_job_state` | `pending`, `queued`, `running`, `held`, `failed`, `skipped`, or `succeeded` |
| `input`            | `JSONB`          | Set once the run reaches it                                            |
| `output`           | `JSONB`          | Set exactly while `succeeded` or `held`                                |
| `summary`          | `TEXT`           | The one line its row collapses to, 1 to 500 characters; a step that skipped itself says why here |
| `error`            | `TEXT`           | Why its last attempt failed, 1 to 2000 characters                      |
| `log`              | `TEXT`           | What it wrote, every attempt's, the newest 65536 characters            |
| `attempts`         | `INTEGER`        | Claims since its run or a person last queued it                        |
| `available_at`     | `TIMESTAMPTZ`    | Set exactly while `queued`: the earliest it may be claimed             |
| `claim_token`      | `UUID`           | Set exactly while `running`, new at every claim; fences the attempt's writes |
| `lease_expires_at` | `TIMESTAMPTZ`    | Set exactly while `running`: when the claim lapses unless renewed      |
| `started_at`       | `TIMESTAMPTZ`    | When its last attempt started                                          |
| `finished_at`      | `TIMESTAMPTZ`    | When its last attempt ended                                            |
| `updated_at`       | `TIMESTAMPTZ`    | Maintained by trigger                                                  |

### `model_settings`

The model and effort an agent runs on, one row per agent an admin set: a workflow step whose work is an agent turn,
by its step name, or an agent outside the step framework, such as `elysia` or `turn_summary`. An agent without a row
follows its kind (`kind_model_settings`, then the kind's default in `api/src/agent_models.rs`); choosing what it would
follow removes the row. See `docs/engine.md`, Models.

| Column       | Type          | Notes                                                                       |
|--------------|---------------|-----------------------------------------------------------------------------|
| `agent`      | `TEXT`        | The step's or agent's name in code, `snake_case`, primary key               |
| `model`      | `TEXT`        | What the CLI's `--model` takes; starts with a letter or digit, so never a flag |
| `effort`     | `TEXT`        | `low`, `medium`, `high`, `xhigh`, or `max`; text, so a new level needs no enum migration |
| `created_by` | `UUID`        | The admin who first set it                                                  |
| `created_at` | `TIMESTAMPTZ` | When it was first set                                                       |
| `updated_at` | `TIMESTAMPTZ` | Maintained by trigger                                                       |

### `kind_model_settings`

The model and effort a kind of agent runs on: one row per kind an admin set for the workspace, and one per kind a
project overrides for its own agents. A kind without a row follows the level beneath it (`docs/engine.md`, Models);
choosing what it would follow removes the row. Unique per `(project_id, kind)` with nulls not distinct, so the
workspace has one row per kind too.

| Column       | Type          | Notes                                                                       |
|--------------|---------------|-----------------------------------------------------------------------------|
| `id`         | `UUID`        | UUIDv7, primary key                                                         |
| `project_id` | `UUID`        | The project it overrides, cascading; `NULL` for the workspace's setting      |
| `kind`       | `agent_kind`  | `worker`, `project_manager`, `correction`, `segmenting`, `triage`, `elysia`, or `summaries`; with a project, only `worker`, `project_manager`, or `summaries` |
| `model`      | `TEXT`        | As `model_settings.model` checks                                            |
| `effort`     | `TEXT`        | As `model_settings.effort` checks                                           |
| `created_by` | `UUID`        | Who first set it: an admin for the workspace's, whoever edited the project otherwise |
| `created_at` | `TIMESTAMPTZ` | When it was first set                                                       |
| `updated_at` | `TIMESTAMPTZ` | Maintained by trigger                                                       |

### `workflow_run_events`

A run's history: its start, and every step a person retried, skipped, or resumed. Shaped like `page_events`.

| Column        | Type          | Notes                                                                  |
|---------------|---------------|------------------------------------------------------------------------|
| `id`          | `UUID`        | UUIDv7, primary key, so entries order as written                       |
| `run_id`      | `UUID`        | References `workflow_runs`, cascading                                  |
| `step_job_id` | `UUID`        | The step it was about, cascading; `NULL` for the run's own entries     |
| `kind`        | `TEXT`        | `started`, `retried`, `skipped`, or `resumed`                          |
| `created_by`  | `UUID`        | Who did it                                                             |
| `data`        | `JSONB`       | Shaped by `kind`; see `docs/engine.md`, A person's actions             |
| `created_at`  | `TIMESTAMPTZ` | When it was done                                                       |
