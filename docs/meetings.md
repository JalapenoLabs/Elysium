# Meetings

Elysium reads a person's meetings, their recordings and timestamped transcripts, as that person, and cuts each one
into topic segments. Each segment lands on the timeline (`docs/connectors.md`) as its own record: the recording
trimmed to it, its transcript excerpt, and a summary, attributed to the projects it is about. So when a two hour
meeting spends five minutes on a product, the product's timeline shows that five minute segment.

Meetings come from meeting sources. Today one exists, Fireflies.ai, read with the person's own API key. The code is
`api/src/meetings/`.

## Meeting sources

A meeting source is a connector whose kind provides meetings: its definition says so (`Provides::Meetings`,
`docs/connectors.md`), and nothing else names it. It reads as its account's person, and installs nothing into an
organization. Like every connector, it is added, edited, checked, paused, and removed under Settings, Connectors alone.

Every source fills one trait, `MeetingSource` in `api/src/meetings/source.rs`: it lists the meetings that started
since a time, and fetches one meeting's transcript with a fresh address to download its recording from.
`api/src/meetings/sources.rs` is the one place a kind meets its source (`Sources::for_kind`), so the loop below lists
and fetches every source the same way, and a test fails when a kind that provides meetings has no source. A new source
adds its kind (`docs/connectors.md`, Adding a kind), declares that it provides meetings, and adds its client and its row
in `for_kind`; the meetings page and the loop pick it up unchanged.

| Source    | Signs in with                                  | Reads                                                    |
|-----------|------------------------------------------------|----------------------------------------------------------|
| Fireflies | The person's API key, from Fireflies' Integrations page | `api.fireflies.ai/graphql`: the transcript listing, then each meeting's sentences, video or audio |

A Fireflies key reads what its own user can see. Recordings need a Pro plan or higher, and video needs the "record
meeting video" setting in Fireflies; without them a meeting is processed from its transcript alone and its segments
carry no clip. Recording addresses expire after a day, so each is asked for right before its download. Fireflies
allows 500 to 5,000 calls a day below Business; listing every ten minutes and fetching each meeting once stays far
inside that.

A sentence's start and end are read as seconds, sent either as a number or a numeric string. That is checked against
the documented schema and a fixture, not yet against a live account.

### Accounts

A Fireflies key is an account of its own (`token_accounts`, `docs/connectors.md`), sealed like every application
secret (`docs/secrets.md`) and bound to its row. The Fireflies connector owns it, made and removed with it, and
names the storage location recordings are kept in. A key is checked with Fireflies (`user { email }`) before anything
is stored, so a key Fireflies refuses is never saved.

Every Fireflies key has its connector. The migration `2026-11-04-010000-0000_give_every_fireflies_key_a_connector`
gave each key whose connector had been removed a paused Fireflies connector of its own, so its person resumes it from
Connectors without entering the key again. Such a connector names no storage location until its person picks one on
its page, and until then its recordings are not kept. When the person had one such key, the meetings its removed
connector read are its connector's again, so resuming it reads none of them twice.

Every meeting belongs to a connector (`connectors`), the same table mailboxes use. `UNIQUE (connector_id, external_id)`
on both `meetings` and `timeline_records` is what keeps a meeting, and each of its segments, from being recorded twice.
A listing that stored meetings, and a meeting that finished or failed, is a run of its connector, and a connector is
`syncing` while one of its meetings is processed.

## Processing

`Meetings::start` runs one loop until shutdown.

- **Listing**, every ten minutes (`POLL_INTERVAL`): each active connector of a kind that provides meetings lists,
  through its kind's source, the meetings that started since its cursor (`{"since": <the newest start time seen>}`),
  at most 50 at a time (`LISTING_SIZE`). A new connector's first listing reaches back two weeks
  (`FIRST_LISTING_DAYS`), so it arrives with its recent meetings but not its whole history. Each meeting the connector
  has not seen is stored `pending`, with its kind as `source_kind`. A key its source refuses marks the connector
  `signed_out` with the reason in `last_error`.
- **Processing**, every minute (`PROCESS_INTERVAL`): each unfinished meeting is carried through its steps until it is
  done or one fails.

| Step        | From        | What happens                                                                      |
|-------------|-------------|-----------------------------------------------------------------------------------|
| Record      | `pending`   | Fetch the transcript and the recording's address from the meeting's source, download the recording into the workspace, upload it to the connector's storage location at `meetings/<id>/recording.<ext>` |
| Segment     | `recorded`  | Elysia reads the transcript in one text turn and answers the topic segments, each with its projects |
| Publish     | `segmented` | `ffmpeg` cuts each segment's clip in the workspace, each is uploaded to `meetings/<id>/segments/<n>.<ext>`, and every segment becomes a timeline record in one transaction |

A step that fails records why on the meeting (`last_error`, `attempts`) and is tried again on the next pass. After
three failures (`MAX_ATTEMPTS`) the meeting is `failed`; `POST /api/v1/meetings/{id}/retry` resumes it from the step
it failed at. Every step is safe to repeat: storage paths are fixed per meeting and segment, and records are written
only once every clip is up.

### Segmentation

Segmentation is a text turn on the runner (`TurnMode::Text`, `docs/coding.md`), on `claude-sonnet-5-5`: no tools, no
settings, and nothing kept to resume. Its prompt holds the meeting's title, length, and participants, every project's
id, name, and the start of its description, and the transcript as `[m:ss] Speaker: text` lines. The answer is one
JSON object of segments, each with its start and end in seconds, a title, a summary of what was said and decided, and
`{ projectId, confidence }` pairs.

Nothing in the answer is taken on trust (`api/src/meetings/segmentation.rs`):

- A segment is kept only inside the meeting, in order, starting no earlier than the last one ended, and 15 seconds
  long at least. At most 40 are kept.
- An attribution is kept only for a project that was offered, once per segment, with its confidence clamped to 0 to 1,
  and dropped below 0.3.
- An answer with no JSON, the wrong shape, or no segment left fails the step.

The runner passes a prompt as one argument to `claude`, and Linux allows one argument 128 KiB. A transcript therefore
merges each speaker's consecutive lines and, past 100 KB, shortens every line evenly; a transcript too long for even
that keeps its start and says where it was cut.

### Clips

`ffmpeg` runs in the workspace container (`workspace/Dockerfile`), so the API's image stays small. Each meeting gets a
scratch directory there, `.elysium/meetings/<id>`, holding its recording while its clips are cut, fetched back from
storage if the workspace lost it, and removed once the meeting is done. Storage is where recordings and clips are
kept.

A clip is cut with `-c copy`: fast and lossless, but it starts at the keyframe nearest the segment's start, a second
or two early at most. MP4-family clips are written with `+faststart`, so a browser plays them as they stream.

Recording downloads use their own HTTP client, which follows redirects only to `https` addresses on a named host,
and the first address is held to the same rule, so a source's address cannot lead Elysium into its own network.

## Segment records

Each segment is a timeline record:

| Field          | Holds                                                                          |
|----------------|--------------------------------------------------------------------------------|
| `source_kind`  | `meeting`                                                                      |
| `external_id`  | `<the meeting's external id>#<segment index>`                                  |
| `occurred_at`  | The meeting's start plus the segment's start                                   |
| `actor`        | The segment's speakers, in the order they first spoke                          |
| `title`        | The segment's title                                                            |
| `body`         | Its transcript excerpt, as `[m:ss] Speaker: text` lines                        |
| `url`          | The meeting's page at its source                                               |
| `media`        | `{ meetingId, startMs, endMs, clip: { storageLocationId, path, contentType } \| null }` |
| `metadata`     | `{ meetingId, meetingTitle, meetingSource, segmentIndex, summary }`            |

Its projects are attributed by `elysia` with her confidence; a person's attribution replaces hers, as for any record.
Each record publishes `timelineRecord.created`, which Elysia's intake hears. `GET /api/v1/timeline/{id}/media`
streams the clip from storage to the record's owner alone.

## In the web app

- **Settings, Meetings** (`/settings/meetings`, in the directory's Knowledge base section, `MeetingsPage`): the
  person's meeting sources and their recent meetings. The page names no source itself; it finds them from the kinds
  whose definitions provide meetings (`GET /connectors/kinds`), so a new source appears on it unchanged.
  - **Sources.** Each of the person's meeting connectors, with its logo, account, and status, linking to its connector's
    page, where it is edited, checked, paused, and removed. Add a source opens the add-connector grid narrowed to the
    kinds that provide meetings (`/settings/connectors/new?provides=meetings`). With no source connected, the page
    says so and shows each kind that provides meetings as a card opening its own form.
  - **Recent meetings.** Each with the source it came from (linking to its connector while there is one), when it was
    and how long, its state, and the reason and a retry for one that failed.
- **A project's timeline** (`ProjectTimeline` on the project page): every record attributed to the project, newest
  first, fifty at a time with "Show older". A meeting segment shows its title, its place in the meeting, its summary,
  its clip (video or audio, loaded only when played), and its transcript excerpt on request. Elysia's attribution
  says how sure she was; the person can confirm it, which makes it theirs, or take the record off the project.
- Records and meetings reach Redux through SWR loaders and stay current through `timelineRecord.created` and
  `meeting.changed`, each loading what it names by id (`docs/frontend.md`).

## What is published

| Event                       | When                                                                     |
|-----------------------------|--------------------------------------------------------------------------|
| `meeting.changed`           | A meeting was read or its processing moved on: its id, state, and owner, never its title or transcript |
| `timelineRecord.created`    | A segment's record was stored                                            |
| `connector.changed`         | A meeting connector was added, a listing changed its sign-in state or error, or a meeting moved it |

## Roadmap

- Google Meet and Teams recordings with their transcripts, read through the person's signed-in browser
  (`docs/browser.md`): Meet's land in Google Drive, Teams' in OneDrive, both reachable as the user. Every source today
  signs in with a key (`Meetings::key_of`); a source read through a browser resolves its sign-in beside its source in
  `sources.rs` instead.
- Fetching each meeting as an engine step, one source per run (`docs/engine.md`), through the same `Sources::for_kind`.
- Passing long prompts to the CLI on standard input, so a transcript never has to be shortened to fit an argument.
- Seeking inside a clip: the media route serves a clip whole and does not answer range requests yet.
- Holding an activity while a meeting's files move, so auto-update waits for a download in progress.
