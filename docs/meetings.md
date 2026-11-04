# Meetings

Elysium reads a person's meetings, their recordings and timestamped transcripts, as that person, and cuts each one
into topic segments. Each segment lands on the timeline (`docs/connectors.md`) as its own record: the recording
trimmed to it, its transcript excerpt, and a summary, attributed to the projects it is about. So when a two hour
meeting spends five minutes on a product, the product's timeline shows that five minute segment.

Meetings come from meeting sources. Today one exists, Fireflies.ai, read with the person's own API key. The code is
`api/src/meetings/`.

Every source is read the same deterministic way: the timestamped transcript, and the recording when there is one.
Nothing a source's own AI wrote (its summary, action items, notes, outline, or analytics) is asked for, stored, or
used, since the model behind it is unknown. Fireflies' queries name each field they read, and a test fails if one asks
for any of Fireflies' AI fields (`api/src/meetings/fireflies.rs`).

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

`Meetings::start` runs one loop until shutdown, and each meeting is also a run of the engine's `meeting` workflow
(`docs/engine.md`, Workflows and steps), which fetches it.

- **Listing**, every ten minutes (`POLL_INTERVAL`): each active connector of a kind that provides meetings lists,
  through its kind's source, the meetings that started since its cursor (`{"since": <the newest start time seen>}`),
  at most 50 at a time (`LISTING_SIZE`). A new connector's first listing reaches back two weeks
  (`FIRST_LISTING_DAYS`), so it arrives with its recent meetings but not its whole history. Each meeting the connector
  has not seen is stored `pending`, with its kind as `source_kind`. A key its source refuses marks the connector
  `signed_out` with the reason in `last_error`.
- **Processing**, every minute (`PROCESS_INTERVAL`): each `pending` meeting's run is started, or followed, and every
  fetched meeting is carried through the loop's steps until it is done or one fails.

| Step        | From        | Where           | What happens                                                    |
|-------------|-------------|-----------------|-----------------------------------------------------------------|
| Fetch       | `pending`   | Its run         | `fetch_meeting` stores the transcript and uploads the recording (Fetching, below), and moves the meeting to `recorded` |
| Segment     | `recorded`  | The loop        | Elysia reads the transcript in one text turn and answers the topic segments, each with its projects |
| Publish     | `segmented` | The loop        | `ffmpeg` cuts each segment's clip in the workspace, each is uploaded to `meetings/<id>/segments/<n>.<ext>`, and every segment becomes a timeline record in one transaction |

### The run and the loop

The workflow is declared in `api/src/meetings/workflow.rs`, and its one step today is `fetch_meeting`. Transcription,
correction, and segmenting join it as steps after the fetch (`docs/engine.md`, Meetings); once segmenting is a step,
the loop's Segment and Publish move into the run. Until then the two share the meeting this way:

- The loop starts a `pending` meeting's run on its pass: one run per meeting (source `meeting`, the meeting's id),
  started by the system user. Starting answers the live run when there is one, so every pass may ask.
- The run's fetch moves the meeting to `recorded`, and the loop takes it from there on its next pass. The loop never
  segments a meeting its run has not fetched.
- A run that fails fails its meeting with the run's reason, recorded as a failed run of its connector, so the Meetings
  page and the connector say why. Its step's own attempts and backoff are the run's (`docs/engine.md`, Runs).
- Retrying a meeting whose fetch failed (`POST /api/v1/meetings/{id}/retry`, `workflow::retry`) also retries its run's
  failed step, as the person who asked, in the same transaction; otherwise the loop would find the same failed run and
  fail the meeting again. A person may retry that step from the run's page instead, and the meeting is fetched all the
  same.

A step of the loop that fails records why on the meeting (`last_error`, `attempts`) and is tried again on the next
pass. After three failures (`MAX_ATTEMPTS`) the meeting is `failed`; the same retry resumes it from the step it
failed at: segmenting for a meeting that was fetched, publishing for one with segments. Every step is safe to repeat:
storage paths are fixed per meeting and segment, and records are written only once every clip is up.

### Fetching

`fetch_meeting` (`api/src/meetings/fetch.rs`) takes `{ meetingId }` and gives `{ meetingId, transcriptHash,
recording }`, the recording being where it is kept (`{ storageLocationId, path, contentType }`) or `null`. One step
reads every source, through the meeting's connector and `Sources::for_kind`, so a new source needs no step of its own.

- It fetches the transcript and a fresh recording address from the source, downloads the recording into the
  workspace, and uploads it to the connector's storage location at `meetings/<id>/recording.<ext>`. Without a
  recording, or with nowhere to keep it, the meeting goes on from its transcript alone, and the log says which.
- The transcript is stored as the source sent it, with its SHA-256 (`meetings.transcript_hash`, the hash of its JSON as
  stored), in one write that only a meeting never fetched takes. Its connector shows as syncing meanwhile.
- A meeting is fetched once. Running the step again for the same meeting, a retried attempt or a later run, fetches
  from the source and compares hashes, but writes and uploads nothing: the first fetch is what every later step read.
  Its summary says whether the source's transcript is unchanged or has changed since. An attempt cut short after its
  upload and before its write uploads the recording again on the next attempt, to the same path.
- A key the source refuses, a meeting it does not have, or a connector that was removed fails the step for good, for
  a person to fix and retry; anything else, such as a source that did not answer, is tried again with the run's
  backoff.
- Every company member can read a run's log, while meetings are their owner's alone, so the log names the source,
  counts, the hash, and storage paths, never a title, a participant, or anything said.

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
- Transcription with a local Whisper, correction against the company doc, and segmenting as steps of the meeting
  workflow (`docs/engine.md`, Meetings), segmenting replacing the loop's Segment and Publish.
- Passing long prompts to the CLI on standard input, so a transcript never has to be shortened to fit an argument.
- Seeking inside a clip: the media route serves a clip whole and does not answer range requests yet.
- Holding an activity while a meeting's clips are cut, so auto-update waits for them; the fetch's download already
  holds one as a step of its run.
