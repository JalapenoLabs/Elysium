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
(`docs/engine.md`, Workflows and steps), which fetches it, transcribes its recording, and corrects its transcript.

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
| Transcribe  | `recorded`  | Its run         | `transcribe_meeting` transcribes the recording with the local Whisper and stores it beside the source's transcript (Transcription, below); the meeting's state does not change |
| Correct     | `recorded`  | Its run         | `correct_transcript` writes a corrected copy beside the raw transcripts, and holds the run for its person's approval when the source or the meeting asks (Correction, below); the meeting's state does not change |
| Segment     | `recorded`  | The loop        | Once the run is through, Elysia reads the corrected copy (or the source's transcript, when the run handed on none) in one text turn and answers the topic segments, each with its projects |
| Publish     | `segmented` | The loop        | `ffmpeg` cuts each segment's clip in the workspace, each is uploaded to `meetings/<id>/segments/<n>.<ext>`, and every segment becomes a timeline record in one transaction |

### The run and the loop

The workflow is declared in `api/src/meetings/workflow.rs`: `fetch_meeting`, `transcribe_meeting`, then
`correct_transcript`. Segmenting joins it as a step after the correction (`docs/engine.md`, Meetings), taking the
corrected copy as its input, and the loop's Segment and Publish move into the run. Until then the two share the meeting
this way:

- The loop starts a `pending` meeting's run on its pass: one run per meeting (source `meeting`, the meeting's id),
  started by the system user. Starting answers the live run when there is one, so every pass may ask.
- The run's fetch moves the meeting to `recorded`. The loop segments a `recorded` meeting only once its run is
  through, that is, once the meeting has no live run (`Meetings::waits_on_run`): segmenting reads what the correction
  hands on, so it waits for the transcription and the correction. A run that failed at either, or is held for
  approval, keeps the meeting `recorded` until a person retries or skips the step on the run's page, or approves the
  corrected transcript. A meeting whose run succeeded, or that has no run, is the loop's.
- The loop segments, and excerpts its records from, the corrected copy the run's correction handed on
  (`workflow::corrected_copy`), or the source's transcript when the correction was skipped or the run ran before the
  workflow corrected meetings.
- A run whose fetch fails fails its meeting with the run's reason, recorded as a failed run of its connector, so the
  Meetings page and the connector say why. Its step's own attempts and backoff are the run's (`docs/engine.md`, Runs).
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

### Transcription

Anthropic offers no transcription of recorded audio, and a recording never leaves the host, so Elysium transcribes it
itself: faster-whisper running Whisper large-v3, in int8 on the host's own CPUs. `transcribe_meeting`
(`api/src/meetings/transcribe.rs`) takes what the fetch gives and gives it back with `whisperTranscriptHash`, the
SHA-256 of the Whisper transcript it stored. What the fetch gives reads as that with no hash, so a person may skip the
step and the next one still reads its input.

- It streams the recording from the connector's storage location into Whisper's container. Storage, not the workspace,
  is where a recording is kept, and the loop removes the workspace's copy once a meeting's clips are cut.
- The transcript is stored once, in `meeting_transcriptions`, beside the source's (`docs/database.md`): every segment
  Whisper decoded with its text, and every word in it with its probability, each timed in whole milliseconds from the
  recording's start as the source's lines are. A word keeps its leading space, so a segment's words joined with nothing
  between them read as the segment. Whisper tells no speakers apart. Correction reads it against the source's
  transcript; segmenting cuts clips by its word timings.
- Running the step again for a meeting transcribed already answers the stored transcript and transcribes nothing.
- As a running step, a transcription holds an activity, so auto-update waits for it rather than cutting it short.
- A meeting without a recording (the source keeps none, or the connector names nowhere to keep it) skips the step,
  saying "No recording, so nothing to transcribe", and one whose recording has no audio stream skips it saying that.
  A step skipped this way gives nothing, so the next one receives the fetch's output (`docs/engine.md`, A step).
- Storage or Docker not answering, or the container stopping on its own, is tried again with the run's backoff. A
  recording Whisper cannot decode, a transcription that runs out of memory, one that passes six hours
  (`TRANSCRIPTION_TIMEOUT`), and a transcript Elysium cannot read fail the step for good, for a person to retry or skip.
- The log carries Whisper's progress every five minutes of audio, the language it heard, durations, counts, and the
  hash, never a word of what was said.

#### The container

Each transcription runs in a container of its own (`api/src/whisper/`), which the engine creates through
`docker-proxy`, the way it runs the workspace, the mail server, and browsers, and removes when the transcription ends.
The recording goes in on the container's standard input and the transcript comes out on its standard output as one
JSON document, both over Docker's attach stream, with progress on standard error (`whisper/transcribe.py`). So the
container needs no network at all and shares no volume with anything.

A container per transcription, rather than a compose service the engine calls: a service would need a network to be
called over and would hold the model's memory between meetings, and a container that keeps nothing leaves nothing half
done when it is stopped. Loading the model takes two or three seconds, nothing beside an hour of audio.

| Limit      | Value                                       | Why                                                   |
|------------|---------------------------------------------|-------------------------------------------------------|
| Network    | none                                        | Nothing it holds may leave the host                   |
| CPUs       | its threads (below)                         | It decodes on exactly that many                       |
| CPU weight | 256, a quarter of Docker's default          | The API, Postgres, and agents' turns win under contention |
| Memory     | 12 GiB, no swap                             | The model took 5.0 GiB at its peak, and an hour of audio adds about 230 MB |
| Processes  | 256                                         | Its threads, and nothing like a fork bomb             |
| Privileges | every capability dropped, no new privileges | It reads a stream and does arithmetic                 |

A container is named for its step's job (`elysium-whisper-<id>`), and an attempt first removes any container by that
name, so one an engine left running when it stopped is removed by the attempt that follows. An attempt cut short while
its engine still runs removes its own in the background.

#### CPUs

A transcription decodes on half the host's logical CPUs, at most 16 (`transcription_threads`), and its container is
capped at as many: on a host with two threads per core that is about one per physical core, past which matrix work
gains little, and the other half stays with the API, the engine, Postgres, and agents' turns. On the 32 vCPU
production VM it is 16. One transcription runs at a time per engine; a second waits for the first, saying so in its
log, since two would only share the same cores. The engine logs the thread count it chose as it starts
(`whisper.configured`).

#### The image

`whisper/Dockerfile` builds `jalapenolabs/elysium-whisper`, published with each release like the workspace image, and
the engine runs the one of its own release (`WHISPER_IMAGE`). The development and checkout stacks build it as
`elysium-whisper:dev` and `elysium-whisper:local` (service `whisper-image`). It is about 3.7 GB, 3.1 GB of it the model.

- The base image, `python:3.13.18-slim-trixie`, is pinned by digest.
- The model is `Systran/faster-whisper-large-v3`, Whisper large-v3 converted for CTranslate2, at Hugging Face revision
  `edaa852ec7e145841d8ffdb056a99866b5f0a478`. Each of its five files is added with `ADD --checksum`, which refuses a
  file whose SHA-256 differs and makes the checksum the layer's cache key, so a rebuild never downloads it again. It
  is the first layer after the base, so a change to anything else never rebuilds or pushes it.
- faster-whisper 1.2.1 and every package it needs are installed from wheels pinned by hash
  (`whisper/requirements.txt`). PyAV stays on 18, since faster-whisper 1.2.1 opens recordings with an argument PyAV 19
  removed. PyAV carries FFmpeg's libraries, so any recording the source keeps, video included, is decoded as it is.
- Decoding uses faster-whisper's defaults (beam search of 5, the language detected), with timestamps per word and the
  voice activity filter on, which skips silence, where Whisper otherwise invents text.
- The Hub client is offline and ONNX Runtime's telemetry is off, though the container has no network to use either.

#### How long it takes

`whisper/measure.sh` transcribes a recording under the engine's limits, under `nice`, and prints the host, the time per
hour of audio, and the peak memory. Without arguments it uses the engine's thread count for the host and LibriVox's
public-domain reading of Federalist No. 10 (18:15), downloaded and checked against its SHA-256.

| Host                                             | Threads | Audio | Wall time | Per hour of audio | Peak memory |
|--------------------------------------------------|---------|-------|-----------|-------------------|-------------|
| Development: AMD Ryzen 9 7950X (16 cores, 32 threads), 125 GiB, kernel 7.2.8, 2026-11-04 | 4 | 18:15 | 11:00 | 36 minutes (1.66 times real time) | 5.0 GiB |
| Production VM: Intel Xeon @ 2.80GHz (n2-highcpu-32, 32 vCPUs), 31 GiB, kernel 7.2.14, 2026-11-04 | 16 | 18:15 | 8:30 | 28 minutes (2.15 times real time) | 7.1 GiB |

To measure the production VM after a deploy that built `elysium-whisper:local`, from the deployment's checkout:

```sh
whisper/measure.sh
```

### Correction

Transcription mishears names, products, and jargon, and every source mishears them differently, so Elysium corrects
each meeting's transcript against everything it knows before acting on it. `correct_transcript`
(`api/src/meetings/correct.rs`; the prompt and the checks on the answer are `correction.rs`) takes what the
transcription gives and gives it back with `correctedTranscriptHash`, naming the corrected copy, and `approved`, whether
its person had approved it when the run went on. What the transcription gives reads as that with no copy, so a person
may skip the step and the next one still reads its input. The raw transcripts are never written.

- **What it reads.** Every transcript the meeting has: the source's lines and, when the recording was transcribed,
  Whisper's. The company doc, with its pending proposals (`company_doc::read`, `docs/knowledge.md`), its sections in
  the order correction needs them (people, glossary, products, projects, stakeholders, leadership, company) within
  40,000 bytes, the lines a pending proposal would add marked as proposed. And the company's other meetings: the fixes
  their 30 newest corrections made, those made in the most meetings first, approved corrections first, and the names
  speakers went by in the 30 newest meetings, as their sources labelled them, which is usually how people spell
  themselves. Other meetings are anyone's in the company: the names and terms a company uses are its own, as its
  knowledge base is (`docs/knowledge.md`, Who sees what).
- **What it follows.** The copy follows one transcript line by line: the source's, which names who spoke, or
  Whisper's segments when the source sent no lines. A meeting with neither skips the step, saying "Nothing was said,
  so nothing to correct".
- **Turns.** Each is a text turn on the runner on the step's model setting, Opus 5.5 at high effort unless an admin
  chose another (`docs/engine.md`, Models). Its prompt holds the meeting's title and participants, the doc, what
  other meetings taught, and as many of the transcript's numbered lines as fit under 120,000 bytes, beside Whisper's
  text over the same stretch, five seconds either side. A long meeting takes several turns, each with the doc again,
  since the runner passes the prompt as one argument; at most eight, and lines past them stay as heard, the log says
  from which.
- **The answer** is `{ "edits": [{ "line", "text" }] }`: the whole corrected text of each line it changes.
- **Nothing in the answer is trusted.** The copy is built from the transcript with only the edited lines' text
  replaced, so it keeps every line, in order, with its speaker and times, whatever the model answers. An edit is kept
  only for a line its turn offered, once per line, with text, and only when it fixes words: compared word by word,
  without regard to case or the punctuation around a word, each change replaces at most four words with at most four
  (a name heard as several words is one fix), none adds words where there were none or drops words without a
  replacement, and the words replaced are at most half the line's (or four, on a short line). A refused edit leaves
  its line as heard, and is logged by its line number and why. An answer with no JSON, or the wrong shape, fails the
  attempt, which the run's backoff tries again.
- **Once.** The copy is stored in `meeting_corrections` (`docs/database.md`), once: an attempt that finds it answers
  it without a turn, so an engine that stopped after storing it costs nothing more.
- **What it logs.** Line counts, line numbers, the doc's sections that did not fit whole, and hashes, never a word
  that was said.

#### The corrected copy

What the steps after correction read, segmenting first. The step's output names it by its hash, and
`correct::corrected_lines` reads the copy an output names.

| Field            | Holds                                                                                     |
|------------------|-------------------------------------------------------------------------------------------|
| `transcript`     | An array of `{ startMs, endMs, speaker, text }`, the shape of `meetings.transcript`: one per line of the transcript it follows, in its order, with its speaker and times; only `text` differs |
| `transcript_hash`| The SHA-256 of `transcript` as stored, which the output's `correctedTranscriptHash` names  |
| `based_on`       | `source` or `whisper`: the transcript it follows                                          |
| `changed_lines`  | How many lines it changed                                                                 |
| `fixes`          | Each distinct fix, `{ heard, written }`, which later corrections learn from               |
| `approved_by`, `approved_at` | Who approved it, and when                                                     |

Its times are milliseconds from the recording's start, as Whisper's words are (Transcription, above), so a stretch of
the copy maps onto Whisper's word timings by time alone: a step cuts a segment's clip at the words nearest its ends.

#### Waiting for approval

- Each meeting source has "Wait for my approval before acting on meetings", off by default: the `waitForApproval`
  toggle in its connector's content, on the connector's page (`docs/connectors.md`). A person can also hold one
  meeting by hand from its page, until it is corrected (`meetings.hold_for_approval`); once the copy is stored,
  approving it is what remains, and holding answers `409 meeting_corrected`.
- When either asks and the copy is not approved, the step holds the run (`Done::Held`, `docs/engine.md`, A step) and
  raises an inbox item, "Approve the corrected transcript of" the meeting's title, as the system, recording it on the
  copy in the same transaction, under the meeting's lock, so an attempt that runs again raises no second. Items are
  the whole workspace's, so it carries the meeting's title, as an item a person's mail raises carries its subject,
  and nothing that was said.
- Approving, from the meeting's page, records who approved it, resolves the item, and resumes the held run with the
  step's output saying `approved: true` (`step_job::resume_with`), in one transaction. Resume on the Runs page passes
  the held output on as it is, unapproved, and leaves the item for its person.
- Otherwise approval is optional: a copy nobody waits for goes on unapproved, and approving it later records the
  approval on the copy.

### Segmentation

Segmentation is a text turn on the runner (`TurnMode::Text`, `docs/coding.md`), on the `segment_meeting` model
setting, Opus 5.5 at high effort unless an admin chose another (`docs/engine.md`, Models): no tools, no
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
streams the clip from storage to the record's owner alone. Each segment is a document of the knowledge index, stored
with its record: its title, its summary, and its excerpt, never its clip (`docs/knowledge.md`, Timeline records).

## In the web app

- **Settings, Meetings** (`/settings/meetings`, in the directory's Knowledge base section, `MeetingsPage`): the
  person's recent meetings and their meeting sources, as two tabs with their counts, the open one in `?tab=`. The page
  names no source itself; it finds them from the kinds whose definitions provide meetings (`GET /connectors/kinds`), so
  a new source appears on it unchanged.
  - **Sources.** Each of the person's meeting connectors, with its logo, account, and status, linking to its connector's
    page, where it is edited, checked, paused, and removed. Add a source opens the add-connector grid narrowed to the
    kinds that provide meetings (`/settings/connectors/new?provides=meetings`). With no source connected, the tab
    shows each kind that provides meetings as a card opening its own form, and an empty Recent meetings tab links to
    it.
  - **Recent meetings.** A table (`MeetingTable`) whose search, Columns, and export sit in the page's titlebar: each
    meeting's title, opening its page; the source it came from, linking to its connector while there is one; its
    state, with why it failed, that its run waits for the person's approval, or that a finished one was published
    from its transcript alone; when it started, how long it ran, and how many topics Elysia found. A failed meeting's
    row has Retry, which resumes it from the step it failed at.
- **A meeting's page** (`/settings/meetings/:meetingId`, `MeetingPage`): its title, source, and state; when it
  started, how long it ran, and a link to it at its source, in a new tab; where its correction stands (not corrected
  yet, waiting for the person's approval, approved and by whom, or not approved); and its transcript
  (`MeetingTranscript`, `GET /meetings/{id}/transcript`): as heard beside its corrections, as a `TextDiff` whose
  unchanged lines fold away, or as heard until it is corrected or when nothing needed it. Approve is the header's
  primary action while the copy waits for one. Its Actions hold the meeting for the person's approval, or stop
  holding it, until it is corrected and unless its source already waits, and open its run.
- **An item asking for a review** links to its meeting's page, from the item's page (`MeetingReviewLink`, reading
  `GET /meetings?reviewItemId=`).
- **The source's setting** is its connector's content form's switch, "Wait for my approval before acting on
  meetings", saved with the page.
- **A project's timeline** (`ProjectTimeline`, the project page's Timeline tab, which counts every record): every
  record attributed to the project, newest first, fifty at a time with "Show older". A meeting segment shows its
  title, its place in the meeting, its summary, its clip (video or audio, loaded only when played), and its
  transcript excerpt on request. Elysia's attribution
  says how sure she was; the person can confirm it, which makes it theirs, or take the record off the project.
- Records and meetings reach Redux through SWR loaders and stay current through `timelineRecord.created` and
  `meeting.changed`, each loading what it names by id, and `workflowRun.changed` for a meeting's run, which reloads
  the meeting so its page and row say when the run holds for approval (`docs/frontend.md`). A meeting's transcript is
  read again once its correction lands.

## What is published

| Event                       | When                                                                     |
|-----------------------------|--------------------------------------------------------------------------|
| `meeting.changed`           | A meeting was read, its processing moved on, its corrected copy was stored or approved, or its hold changed: its id, state, and owner, never its title or transcript |
| `timelineRecord.created`    | A segment's record was stored                                            |
| `connector.changed`         | A meeting connector was added, a listing changed its sign-in state or error, or a meeting moved it |

## Roadmap

- Google Meet and Teams recordings with their transcripts, read through the person's signed-in browser
  (`docs/browser.md`): Meet's land in Google Drive, Teams' in OneDrive, both reachable as the user. Every source today
  signs in with a key (`Meetings::key_of`); a source read through a browser resolves its sign-in beside its source in
  `sources.rs` instead.
- Segmenting as a step of the meeting workflow after correction (`docs/engine.md`, Meetings), taking the corrected
  copy as its input, replacing the loop's Segment and Publish, and cutting clips by the Whisper transcript's word
  timings.
- Passing long prompts to the CLI on standard input, so a transcript never has to be shortened to fit an argument,
  and a long meeting's correction reads the company doc once rather than in every turn.
- Seeking inside a clip: the media route serves a clip whole and does not answer range requests yet.
- Holding an activity while a meeting's clips are cut, so auto-update waits for them; the fetch's download already
  holds one as a step of its run.
