# Meetings

Elysium reads a person's meetings, their recordings and timestamped transcripts, as that person, and cuts each one
into topic segments. Each segment lands on the timeline (`docs/connectors.md`) as its own record: the recording
trimmed to it, its transcript excerpt, and a summary, attributed to the projects it is about. So when a two hour
meeting spends five minutes on a product, the product's timeline shows that five minute segment.

Meetings come from meeting sources: Fireflies.ai, read with the person's own API key on a paid plan or through
Fireflies' own web app in a browser the person signed in to on any plan, and Microsoft Teams and Slack's huddles, each
read through the browser the person's connector signed in to. The code is `api/src/meetings/`.

A meeting is the one event that happened; each of its captures is one record of it, as its source gave it (Meetings
and captures, below). Its final transcript is a stream of words with stable ids, and every mark on it is a range of
those words (The final transcript, below). Every change to a meeting is in its history, and undoable (History, below).
How meetings become reviewed documents of the knowledge base is designed in `docs/meeting-review.md`.

Every source is read the same deterministic way: the timestamped transcript, and the recording when there is one.
Nothing a source's own AI wrote (its summary, action items, notes, outline, or analytics) is asked for, stored, or
used, since the model behind it is unknown. Fireflies' queries, both ways, name each field they read, and a test fails
if one asks for any of Fireflies' AI fields (`api/src/meetings/fireflies.rs`); Teams' page script reads the recording
and its transcript alone, never the meeting recap or notes Microsoft's AI writes, and a test fails if it names them
(`api/src/connectors/teams/meetings.rs`); Slack's huddles are read from their transcript file alone, never Slack's AI
notes beside it (`api/src/connectors/slack/meetings.rs`).

## Meeting sources

A meeting source is a connector whose kind provides meetings: its definition says so (`Provides::Meetings`,
`docs/connectors.md`), and nothing else names it. It reads as its account's person, and installs nothing into an
organization. Like every connector, it is added, edited, checked, paused, and removed under Settings, Connectors alone.

Every source fills one trait, `MeetingSource` in `api/src/meetings/source.rs`: it lists the meetings that started
since a time, and fetches one meeting's transcript with a fresh address to download its recording from, each as its
person, signed in the way its connector signs in (`MeetingSource::SignIn`). `api/src/meetings/sources.rs` is the one
place a kind meets its source (`Sources::for_kind`), and where each source's sign-in is resolved from its connector
(`Source::sign_in`): a key-based source opens the token account it reads through, and a browser source finds its
connector's browser. The loop below and the fetch step hold the signed-in source (`SignedIn`) and never handle a
credential, so they list and fetch every source the same way, and a test fails when a kind that provides meetings has
no source. A new source adds its kind (`docs/connectors.md`, Adding a kind), declares that it provides meetings, and
adds its client, its sign-in, and its row in `for_kind`; the meetings page and the loop pick it up unchanged.

A source may read meetings only some of the time (`Source::reads`): Fireflies reads nothing else, so always, either
way it signs in, while a Teams connector also reads chats, and reads meetings only while its person leaves them on and
once it names a storage location, and a Slack connector reads huddles while its person leaves them on, with no storage
location, since a huddle keeps no recording. The loop lists no meetings for a connector that reads none, and records nothing on it.

| Source              | Signs in with                                  | Reads                                                    |
|---------------------|------------------------------------------------|----------------------------------------------------------|
| Fireflies           | The person's API key, from Fireflies' Integrations page | `api.fireflies.ai/graphql`: the transcript listing, then each meeting's sentences, then its video or audio |
| Fireflies (browser) | The connector's own browser, signed in to `app.fireflies.ai` | The web app's `/api/v4/hive`, from its tab: the notebook's listing, then each meeting's captions and, apart, its video or audio |
| Teams               | The Teams connector's own browser, signed in to Microsoft | Teams' own store for the listing, then the recording's drive item on the organizer's `SharePoint`: its download and its transcript |
| Slack               | The Slack connector's own browser, signed in to `app.slack.com` | The web client's own methods in the signed-in tab: `huddles.history`, then each huddle's transcript file (`huddles.get`, `files.info`) |

A Fireflies key reads what its own user can see. Each meeting is fetched in two calls: its sentences, then its
recording addresses. Recording addresses need a Pro plan or higher, and video needs the "record meeting video" setting
in Fireflies. On the free plan Fireflies answers the second call with its `paid_required` error code, which Elysium
reads as a meeting without a recording, logging `meetings.fireflies.recording.paid_plan_required` with the plan tier
Fireflies names: the meeting is processed from its transcript alone and its segments carry no clip. Any refusal of
the sentences still fails the fetch. Recording addresses expire after a day, so each is asked for right before its
download. Fireflies allows 500 to 5,000 calls a day below Business; listing every ten minutes and fetching each
meeting once, in two calls, stays far inside that.

A sentence's start and end are read as seconds, sent either as a number or a numeric string. That is checked against
the documented schema and a fixture, not yet against a live account.

### Fireflies through a browser

Fireflies' free plan refuses transcripts and recordings to its API, but its web app reads every meeting, on any plan,
from its own GraphQL endpoint, `POST /api/v4/hive` on `app.fireflies.ai`, signed in by the session in the page. The
`fireflies_browser` kind reads that endpoint from its own browser (`docs/browser.md`), started at Fireflies' sign-in
page; its person signs in once on the connector's page. `api/src/meetings/fireflies_browser/` is the source:
`hive.js` runs as the page over DevTools and asks the endpoint the queries the source hands it, refusing anything
that is not a query, and the web app's session token (`localStorage.AUTHORIZATION`) never leaves the page; only
Fireflies' answers do. The queries are the web app's own, read from its public JavaScript, with selections of Elysium's
own that leave out every AI field; how to find them again is in that directory's `CLAUDE.md`.

- **Listing** asks `getFilteredMeetings` for the meetings the person is part of (the notebook's "My Meetings",
  `USER_PART_OF`) that started since the cursor, oldest first, at most 50, so a listing that cannot hold them all
  leaves the rest for the next one. Fireflies lists a meeting before its words exist, and a meeting is fetched once,
  so a listing stops at the first meeting still being transcribed and the cursor waits before it; one that has said so
  for a day (`TRANSCRIBING_PATIENCE`) is taken as it is, so it never holds the rest back. Each meeting links to its page,
  `app.fireflies.ai/view/<id>`.
- **Fetching** asks `meetingNote` twice in one evaluation: its captions, each with a speaker and start and end in
  seconds, then its video and audio addresses, so a refusal of the recording costs only the recording. The edits its
  people saved in Fireflies are applied as Fireflies shows them: a rewritten sentence in place of what was heard, a
  deleted one left out, one reassigned under its new speaker, and captions with neither a letter nor a digit hidden.
  A speaker is named from `speakerMeta`, or `Speaker <n>` as the web app shows one nobody named. A meeting still being
  transcribed is refused for the run's backoff to try again.
- **Signed in** is the session token in the page, so the check asks nothing of Fireflies. No Fireflies tab opens one
  and reads as not open yet; a tab on Google's, Microsoft's, or Apple's sign-in reads as signed out, as does a session
  Fireflies answers `UNAUTHENTICATED`, which marks the connector `signed_out`.
- The web app renews its own session, a two-week token, as it loads, and Elysium never renews it itself: the daily
  browser refresh reloads the Fireflies tab (`docs/browser.md`, Refreshing), and every read opens one when none is.
- Its storage location is optional (`docs/connectors.md`, Connectors): a meeting with nowhere to keep its recording
  goes on from its transcript alone.

All three queries were checked against a free-plan account's signed-in tab, production's connector browser among
them: the listing, full transcripts, and a recording query that answers audio alone (`video_url` is `null`,
`audio_url` a signed address on `cdn.fireflies.ai`), which downloads whole from the host, outside the browser, so a
free-plan meeting keeps its recording, its clips, and its Whisper transcript.

### Teams

A Teams connector (`docs/connectors.md`, Teams) reads its person's recorded meetings unless they turn off "Read meeting
recordings and transcripts" (`readMeetings`, on by default), and only once they pick the storage location its
recordings are kept in: a browser connector is made before its person can choose one, and a meeting is fetched once, so
reading before then would keep none of those recordings. `api/src/connectors/teams/meetings.rs` reads through
`api/src/connectors/teams_meetings.js`, which runs in the browser's Teams tab as the page, over DevTools, and does only
GETs:

- **Listing** reads the store Teams keeps in IndexedDB (`Teams:replychain-manager:...`), as the chat reader does, and
  calls nothing. Once a meeting's recording is ready, Teams posts a message in the meeting's chat
  (`RichText/Media_CallRecording`, its `RecordingStatus` `Success`, earlier versions `Initial` and `ChunkFinished`):
  the recording's title, start (`RecordingContent timestamp`), length (`duration`, `H:MM:SS.ff`), the call it belongs
  to, and its drive item in the organizer's OneDrive (`item type="onedriveForBusinessVideo"`, with its sharing link,
  `driveId`, and `driveItemId`). The call's ended message (`Event/Call`, matched by `callId`) names the organizer's
  UPN and who took part. So the meetings listed are the recorded ones whose chats this person's Teams has stored. Each
  is known by its drive item's address on SharePoint's API,
  `https://<tenant>-my.sharepoint.com/_api/v2.1/drives/<drive>/items/<item>`, its external id; its link is the sharing
  link, which opens it in Stream. The markup is read with patterns, never a parser, which Teams' Trusted Types policy
  refuses.
- **Fetching** reads that drive item with the access token Teams keeps for the SharePoint host in its MSAL cache
  (localStorage), which never leaves the page: `?select=...,@content.downloadUrl` for the recording's
  pre-authenticated download address (`/_layouts/15/download.aspx?...&tempauth=...`, good for about an hour), then
  `/media/transcripts` for the transcript Teams marks default, then its
  `/media/transcripts/<id>/content?format=json`, whose entries carry `speakerDisplayName`, `startOffset`, `endOffset`
  (`HH:MM:SS.fffffff`), and `text`. The WebVTT the same address serves carries no speakers (`<v>` tags), so the JSON
  is read. The fetch step downloads the recording from the address the page handed over, as it does Fireflies', and
  accepts only `https` on a `*.sharepoint.com` host.

Transcript offsets count from the start of the transcript, which starts with the recording to within a few seconds;
clips are cut on the words Whisper heard (Clips), so that drift never reaches a clip. A recording whose organizer
blocked downloads has no download address, and its meeting is processed from its transcript alone, as a source without
a recording is; the `sample_transcript` check says so (`recording_not_downloadable`). A drive item the person cannot
see, or one removed, answers `403` or `404` and fails its fetch for good. Teams renews its SharePoint token as it is
used; one that has expired, or none yet, fails the fetch with its attempts and backoff and says so. Both were read
against a signed-in Teams: its listing found every recorded meeting its chats held, and a fetch read a 40 minute
meeting's 273 transcript entries from seven speakers, and its recording's address, which answered a ranged download
from outside the browser as `video/mp4`.

### Slack huddles

A Slack connector (`docs/connectors.md`, Slack) reads its huddles as meetings unless its person turns off "Read meeting
recordings and transcripts" (`readMeetings`, on by default), from the same workspaces and conversations its messages
come from. `api/src/connectors/slack/meetings.rs` reads through the connector's page script
(`api/src/connectors/slack.js`), so the session never leaves the browser, with the web client's own methods, which are
not Slack's published API:

- **Listing** is `huddles.history` in each workspace, newest first, 50 a page and at most four pages a listing. A huddle
  that ended with a `transcript_file_id` is a meeting, its id `team/huddle`; one still going, or never transcribed, is
  not listed yet. Whether a huddle is new goes by when it ended, not when it started, since the listing cursor is the
  newest start seen: a long huddle that started before a shorter one still arrives once its transcript is there. Its
  title is `Huddle in #channel`, or `Huddle with` who joined for a direct or group huddle; its people are named with
  `users.info` and `conversations.info`, and its link opens its thread in Slack.
- **Fetching** asks `huddles.get` (`huddle_id`) for the huddle's transcript file, then `files.info` with
  `include_transcription`. Where the workspace keeps Slack's AI notes, that file is the notes canvas ("Huddle AI notes
  & transcript"); only its metadata is read, which names the transcript file beside it (`huddle_transcript_file_id`, a
  file of type `huddle_transcript`), and that is read instead. Nothing of the notes is read.
- **A transcript** is `huddle_transcription.lines`: a speaker's user id, a start in milliseconds from the huddle's start
  (`date_start`, seconds), and the words. Slack gives no end, so a line ends where the next begins and the last where
  the huddle ended. Speakers are named with `users.info`; one not found keeps their id.
- **No recording.** Slack keeps none of a huddle, so each is a meeting in words alone: the fetch offers no recording,
  transcription has nothing to transcribe, and its segments carry no clip. A Slack connector names no storage location.

The shapes were read from a signed-in Slack on a plan with huddle notes, where 65 of 144 huddles had a transcript and
each fetch read one transcript file of 200 to 390 lines. A workspace that transcribes huddles without notes, whose
`transcript_file_id` would name the transcript itself, is read the same way but not yet seen.

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

Every capture a source read belongs to a connector (`connectors`), the same table mailboxes use.
`UNIQUE (connector_id, external_id)` on both `meeting_captures` and `timeline_records` is what keeps a meeting, and each
of its segments, from being recorded twice. A capture with no connector or no id at its source, such as an upload,
names its segments' records by its meeting (`meeting:<id>#<index>`), which the unique key cannot hold once; what does
is that segmenting stores a meeting's records under the meeting's lock, in the transaction that marks it done, and
stores nothing for a meeting done, so two attempts at once publish it once. A listing that stored meetings, and a
meeting whose segments were published, is a run of its connector, and a connector is `syncing` while a meeting of its
is fetched or its segments are cut and published.

## Meetings and captures

- **A meeting** (`meetings`, `models::meeting`) is the event: its title, when it started and for how long, who
  organized it and took part, its segments, its hold, and how far its processing got. It belongs to its person, whose
  connector read its first capture.
- **A capture** (`meeting_captures`, `models::meeting_capture`) is one record of it: a connector's meeting today, and
  an upload, a Teams recording, or a Slack huddle on the roadmap (`docs/meeting-review.md`). It keeps what its source
  gave, as it came: its connector, its source and the source's id for it, its page there, the transcript with its
  SHA-256, and the recording with the SHA-256 of its bytes. Both are written once, by its fetch, and never again.
- **One clock.** A capture runs on its own clock, the one its transcript and recording are timed by. Its `offset_ms`
  places that clock on the meeting's: its moment `t` is `t + offset_ms` on the meeting's clock, which starts at the
  meeting's `started_at`. Every time of the final transcript and of a segment's record is on the meeting's clock.
- A listing stores a meeting and its first capture together, in one transaction, so a capture its connector recorded
  before leaves no meeting behind (`meeting::insert_new`).
- A meeting holds one capture until captures can be merged (#238). The steps read the one the run names, and
  everything shown of a meeting's source (its connector, its source, its page there, and whether a recording is kept)
  is its first capture's.
- A recording kept before captures were hashed has no `recording_hash`; it is never downloaded again to hash it.

## Processing

`Meetings::start` runs one loop until shutdown, and every step of a meeting is a step of its run of the engine's
`meeting` workflow (`docs/engine.md`, Workflows and steps), declared in `api/src/meetings/workflow.rs`.

- **Listing**, every ten minutes (`POLL_INTERVAL`): each active connector of a kind that provides meetings lists,
  through its kind's source, the meetings that started since its cursor (`{"since": <the newest start time seen>}`),
  at most 50 at a time (`LISTING_SIZE`). A new connector's first listing reaches back two weeks
  (`FIRST_LISTING_DAYS`), so it arrives with its recent meetings but not its whole history. Each meeting the connector
  has not seen is stored `pending`, with its first capture naming the connector, its kind as `source_kind`, and the
  source's id. A key its source refuses, or a browser signed out, marks the connector `signed_out` with the reason
  in `last_error`. The listing writes only its own `since` into the cursor, merged over what is stored
  (`connector::record_poll`), so a Teams connector's chat reader keeps its own keys beside it; the two share the
  connector's sign-in state and last error, so each read reports the latest it saw.
- **Starting runs**, every minute (`PROCESS_INTERVAL`): each meeting not yet `done` that needs a run gets one, started
  by the system user (`workflow::run_to_start`).

| Step                 | Moves the meeting       | What happens                                                    |
|----------------------|-------------------------|-----------------------------------------------------------------|
| `fetch_meeting`      | `pending` to `recorded` | Stores the capture's transcript and uploads its recording, each with its hash (Fetching, below) |
| `transcribe_meeting` | No                      | Transcribes the capture's recording with the local Whisper and stores it beside the source's transcript (Transcription, below) |
| `correct_transcript` | No                      | Writes a corrected copy beside the raw transcripts, builds the final transcript from it, and holds the run for its person's approval when the source or the meeting asks (Correction, below) |
| `segment_meeting`    | `segmented`, then `done` | Cuts the corrected copy into topics, cuts each one's clip, and publishes every topic as a timeline record attributed to its projects (Segmentation, below) |

Each step's output names the capture it was about (`captureId`), and the step after reads that capture. A step
handed what a run started before meetings had captures gave, which names the meeting alone, reads the meeting's one
capture, logging that it did (`meetings::capture_of`).

### A meeting's run

- **One run carries a meeting.** The loop starts a meeting's run from its fetch when it never had one (source
  `meeting`, the meeting's id), and one that enters the workflow at segmenting (`Workflow::new_run_at`) when it was
  fetched and its newest run ended before segmenting was one of its steps, with what the meeting holds
  (`workflow::run_from_segmenting`): its capture, that capture's transcript hash, recording, and Whisper transcript,
  and its corrected copy with whether it was approved. Any other meeting is its run's, live or not, and the loop
  starts nothing for it.
- **A meeting keeps how far it got, and nothing else.** `meetings.state` is `pending`, `recorded`, `segmented`, or
  `done`, each written by a step. Whether it failed, and why, is its newest run's, read as the meeting is answered
  (`workflow::processing`): a failed run reads as `failed` with the run's reason, and a run that went through without
  publishing it, because a person skipped a step it needed, reads as `skipped`. Nothing copies the run's state onto the
  meeting, so a step retried or skipped from the run's page moves the meeting at once.
- **Skipping.** A step skipped by a person hands the next one its own input (`docs/engine.md`, A step), and every
  meeting step reads its predecessor's input as a meeting not fetched, not transcribed, or not corrected. So skipping
  the fetch skips the meeting: transcription, correction, and segmenting each skip it in turn, the run succeeds, and the
  meeting reads `skipped` with no other run started.
- **Retrying.** Retry on the Meetings page (`POST /api/v1/meetings/{id}/retry`, `workflow::retry`) retries its run's
  failed step as the person who asked, exactly as Retry on the run's page does. Each step's attempts and backoff are
  the run's (`docs/engine.md`, Runs).
- Every step is safe to repeat: what it stores is written once, storage paths are fixed per capture and segment, and a
  meeting's records are written in the transaction that marks it done.

### Fetching

`fetch_meeting` (`api/src/meetings/fetch.rs`) takes `{ meetingId }` and gives `{ meetingId, captureId, transcriptHash,
recording }`, the recording being where it is kept (`{ storageLocationId, path, contentType }`) or `null`. What it
takes reads as what it gives with no hash, a meeting not fetched, which is what the step after receives when a person
skips it. One step reads every source, through the capture's connector and `Sources::for_kind`, so a new source needs
no step of its own. It fetches the meeting's capture, the one its source listed.

- It fetches the transcript and a fresh recording address from the source, downloads the recording into a workspace
  directory of the attempt's own, hashing it as it streams, and uploads it to the connector's storage location at
  `meetings/<meeting>/captures/<capture>/<sha256>.<ext>`, named by its hash. Without a recording, or with nowhere to
  keep it, the meeting goes on from its transcript alone, and the log says which. A source whose plan keeps no
  recording, such as Fireflies' free plan, offers none, so its meetings are fetched from their transcripts alone
  rather than failing.
- Two attempts can run at once (one whose lease lapsed, and the next), and a source may answer each with other bytes.
  Named by its hash, each attempt's recording lands at a path of its own, so the path the capture keeps holds exactly
  the bytes its hash says. Only the attempt whose write the capture keeps moves its download to the capture's scratch
  copy in the meeting's workspace directory, for segmenting to cut clips from; any other discards its own.
- The transcript is stored on the capture as the source sent it, with its SHA-256 (`meeting_captures.transcript_hash`,
  the hash of its JSON as stored), and the recording's place with the SHA-256 of its bytes
  (`meeting_captures.recording_hash`), in one write that only a capture never fetched takes, which moves the meeting to
  `recorded` in the same transaction. Its connector shows as syncing meanwhile.
- A capture is fetched once. Running the step again for the same meeting, a retried attempt or a later run, fetches
  from the source and compares hashes, but writes and uploads nothing: the first fetch is what every later step read.
  Its summary says whether the source's transcript is unchanged or has changed since. An attempt cut short after its
  upload and before its write uploads the recording again on the next attempt, to the same path when the source
  sends the same bytes.
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
- Each capture with audio is transcribed on its own, and its transcript stored once, in `meeting_transcriptions`, under
  the capture, beside its source's (`docs/database.md`): every segment Whisper decoded with its text, and every word in
  it with its probability, each timed in whole milliseconds from the recording's start as the source's lines are, on
  the capture's own clock. A word keeps its leading space, so a segment's words joined with nothing between them read
  as the segment. Whisper tells no speakers apart. Correction reads it against the source's transcript; segmenting
  cuts clips by its word timings.
- Running the step again for a capture transcribed already answers the stored transcript and transcribes nothing.
- As a running step, a transcription holds an activity, so auto-update waits for it rather than cutting it short.
- A meeting without a recording (the source keeps none, or the connector names nowhere to keep it) skips the step,
  saying "No recording, so nothing to transcribe", and one whose recording has no audio stream skips it saying that.
  A step skipped this way gives nothing, so the next one receives the fetch's output (`docs/engine.md`, A step). A
  meeting not fetched skips it saying "Not fetched, so nothing to transcribe".
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
its person had approved it when the run went on, as segmenting's input (`TranscriptToSegment::Meeting`, whose JSON is
`CorrectedMeeting`'s own). What the transcription gives reads as that with no copy, so a person may skip the step and
the next one still reads its input. A meeting not fetched skips it, saying "Not fetched, so nothing to correct". The
raw transcripts are never written.

- **What it reads.** Every transcript the meeting's capture has: the source's lines and, when the recording was
  transcribed, Whisper's. The company doc whole, with its pending proposals (`company_doc::read`,
  `docs/knowledge.md`), its sections in the order correction needs them (people, glossary, products, projects,
  stakeholders, leadership, company), the lines a pending proposal would add marked as proposed. And the company's
  other meetings: the fixes their 30 newest corrections made, those made in the most meetings first, approved
  corrections first, and the names speakers went by in the 30 newest meetings' captures, as their sources labelled
  them, which is usually how people spell themselves. Other meetings are anyone's in the company: the names and terms a company uses are its own, as its
  knowledge base is (`docs/knowledge.md`, Who sees what).
- **What it follows.** The copy follows one transcript line by line: the source's, which names who spoke, or
  Whisper's segments when the source sent no lines. A meeting with neither skips the step, saying "Nothing was said,
  so nothing to correct".
- **The turn.** One text turn on the runner, however long the meeting, on the model resolved for the step, of the
  correction kind: Opus 5.5 at high effort unless an admin chose another for the step or the kind (`docs/engine.md`,
  Models). Its prompt holds the meeting's title and participants, the doc and what other meetings taught, once, then
  every one of the transcript's numbered lines, then every segment Whisper heard, timed on the same clock. The runner
  hands the CLI its prompt on standard input (`docs/coding.md`, How a prompt reaches the CLI), so nothing is cut to
  fit. A meeting too long for the model's context fails the step for good, giving the CLI's reason, since every
  attempt would read the same prompt (`TurnOutcome::prompt_too_long`); skipping the step on the Runs page lets
  segmenting go on from the transcript as heard.
- **The answer** is `{ "edits": [{ "line", "text" }] }`: the whole corrected text of each line it changes.
- **Nothing in the answer is trusted.** The copy is built from the transcript with only the edited lines' text
  replaced, so it keeps every line, in order, with its speaker and times, whatever the model answers. An edit is kept
  only for a line the transcript has, once per line, with text, and only when it fixes words: compared word by word,
  without regard to case or the punctuation around a word, each change replaces at most four words with at most four
  (a name heard as several words is one fix), none adds words where there were none or drops words without a
  replacement, and the words replaced are at most half the line's (or four, on a short line). A refused edit leaves
  its line as heard, and is logged by its line number and why. An answer with no JSON, or the wrong shape, fails the
  attempt, which the run's backoff tries again.
- **Once.** The copy is stored in `meeting_corrections` (`docs/database.md`), once: an attempt that finds it answers
  it without a turn, so an engine that stopped after storing it costs nothing more.
- **The final transcript.** Every attempt that has a copy builds the meeting's final transcript from it, unless the
  meeting has one already (The final transcript, below), so an engine that stopped between the two builds it on the
  next attempt.
- **What it logs.** Line counts, line numbers, and hashes, never a word that was said.

#### The corrected copy

What the steps after correction read, segmenting first. The step's output names it by its hash, and
`correct::corrected_lines` reads the copy an output names.

| Field            | Holds                                                                                     |
|------------------|-------------------------------------------------------------------------------------------|
| `transcript`     | An array of `{ startMs, endMs, speaker, text }`, the shape of `meeting_captures.transcript`: one per line of the transcript it follows, in its order, with its speaker and times; only `text` differs |
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

`segment_meeting` (`api/src/meetings/segment.rs`; the prompts and the checks on the answers are `segmentation.rs`) is
the workflow's last step, and the first of the request workflow (`docs/engine.md`, Conversations). It is named for the
meetings it segments, and its model setting is filed under that name; it takes one of three things to read
(`TranscriptToSegment`, untagged, so each reads as its own JSON):

- **A meeting** past its correction (`CorrectedMeeting`), or what the transcription or the fetch gave when a person
  skipped the steps between.
- **A conversation** (`ConversationToSegment`: `{ title, recordIds }`), messages already on one person's timeline,
  which `crate::conversations` gathers from a direct message, a thread, or a mail thread, and starts a request run
  with once the conversation goes quiet: the messages the run takes and up to 20 before them. `title` is what the
  conversation is called where it was had, such as `#launch in Mooreslab`.
- **An action item** (`ItemToAttribute`: `{ actionItemId }`) that arrived in no project, such as an issue assigned to
  its person, which the `assigned_issue` workflow hands it (`docs/action-items.md`, Projects and the lead).

What it gives (`Segmented`) is every topic in order, each with its title, summary, span, the projects Elysia attributed
it to with her confidence, and its records: a meeting segment's own, or a conversation topic's messages. For a
conversation it also names every record it read (`recordIds`), so a conversation's input, which a person skipping the
step hands on, reads as a `Segmented` with no topics and still names its messages. Steps after it read from that what
landed on which project. Every record it puts on a project wakes that project's Project Manager in the same
transaction (`docs/engine.md`, Project Managers), so a meeting's or a conversation's topics reach the projects they
are about with nothing more for a workflow to do.

**The turn.** One text turn on the runner (`TurnMode::Text`, `docs/coding.md`) through `StepContext::model`, on the
segmenting kind: Opus 5.5 at high effort unless an admin chose another for the step, the kind, or (where a kind runs in
projects) a project (`docs/engine.md`, Models). No tools, no settings, and nothing kept to resume. Its prompt holds
every project's id, name, and the start of its description, and:

| Source       | The prompt holds                                                   | The answer names each segment by |
|--------------|--------------------------------------------------------------------|----------------------------------|
| Meeting      | Its title, length, participants, and transcript as `[m:ss] Speaker: text` lines | Its start and end in seconds |
| Conversation | Its title, the name of the person whose timeline it is on, message count, participants, and messages as `[n] [m:ss] Speaker: text` lines, timed from the first | Its first and last message, numbered from 1 |

A conversation's topics are runs of messages, since a chat's topics are not stretches of time: two messages a few
seconds apart may be two requests. Each answer segment also has a title, a summary of what was said and decided, and
`{ projectId, confidence }` pairs. A conversation's answer also names the promises the person made in it,
`{ message, recipient, promise, projectId }` each: the message that makes it, who it is for, what they will tell them
as a short phrase, and the project whose work keeps it (`docs/engine.md`, Follow-ups).

**Nothing in the answer is taken on trust:**

- A meeting's segment is kept only inside the meeting, in order, starting no earlier than the last one ended, and 15
  seconds long at least. A conversation's is kept only as a run of its messages, starting after the last one ended and
  ending at its last message at most. At most 40 are kept.
- An attribution is kept only for a project that was offered, once per segment, with its confidence clamped to 0 to 1,
  and dropped below 0.3. One to a project removed while the turn ran is left out when it is written.
- A promise is kept only when it names one of the messages shown, once each, someone it is for (at most 200
  characters), what was promised (cut to 500), and a project that was offered. At most ten are kept, and an answer
  naming none still reads.
- An answer with no JSON, the wrong shape, or no segment left fails the attempt, which the run's backoff tries again.

The prompt holds the transcript whole, one line for each of its lines, every message of a conversation whole, and
every project: the runner hands the CLI its prompt on standard input (`docs/coding.md`, How a prompt reaches the CLI),
so nothing is merged or shortened to fit. A transcript too long for the model's context fails the step for good,
giving the CLI's reason, since every attempt would read the same prompt.

**A meeting**, step by step:

1. A meeting not fetched skips the step ("Not fetched, so nothing to segment"), and one already `done` answers what it
   published, changing nothing.
2. It reads the corrected copy its input names (`correct::corrected_lines`), or, when the run handed on none, the
   transcript correction follows: the capture's source lines, or Whisper's segments when the source sent none. A
   meeting whose correction was skipped has its final transcript built here from the same (The final transcript,
   below). A meeting in which nothing was said is `done` with no topics, and the step skips ("Nothing was said, so
   nothing to segment").
3. The answer is stored on the meeting (`meetings.segments`, `segmented`) before anything else, once: an attempt that
   finds it stored cuts and publishes from it without asking again.
4. Each segment's clip is cut and uploaded (Clips, below).
5. Every segment becomes a timeline record (Segment records, below), attributed to its projects as Elysia, and the
   meeting is marked `done`, in one transaction. Then each record is announced, its capture's connector records a run,
   and the workspace's copy of the recording is removed.

The connector shows as syncing meanwhile, and the step holds a `workflowStep` activity as every running step does, so
auto-update waits for clips being cut.

**A conversation:** its records are read (any it names that are gone are left out, and a conversation of more than one
person's records fails for good), ordered by when they were said, and segmented in one turn. Each message is then
attributed to its topic's projects, and the reading replaces what Elysia said of those messages before: an attribution
of hers that it no longer makes is taken off, so a conversation read again as it grows keeps no guess its fuller
reading dropped. A person's attributions are never touched (`docs/connectors.md`, The timeline). Each message whose
attributions changed is announced as `timelineRecord.changed`. In the same transaction each promise it names is
recorded on its project, once per message, when that message is answered through its connector, waking the project's
Project Manager; the log counts the new ones and those made where no follow-up can answer them.

**A conversation's topics are not records.** A meeting needs its segments as records because nothing else of it is on
the timeline; a conversation's messages already are, each attributed to its topic's projects, so a topic record would
show every message on a project's timeline twice. A conversation is also read again as it grows, which merges and
splits its topics, so a topic has no identity a record, and a person's correction of it, could keep. A topic's title
and summary are in the run's output (`Segmented`), which the handling agent reads on its run's first turn and the
steps after segmenting read from.

**An action item has no topics.** One turn on the same model (`segment/item.rs`) reads its title, the start of its
notes, and the issues it follows (each one's provider, key, title, link, and where it was filed) against every project
and where each project's work is: the tracker it files issues in, named or followed from its items' issues
(`docs/engine.md`, Its writes), and the repositories its workers clone, since an issue in a project's tracker or
repository is most likely that project's. The answer is `{ "projects": [{ projectId, confidence }] }`, kept as a
segment's attributions are, surest first; an answer naming none reads as an item no project is about. The item joins
each project as Elysia's, the surest first, so the surest leads it, and whatever anyone else decided about its projects
or its lead stands (`docs/action-items.md`, Projects and the lead). What it gives is one topic, titled as the item, with
the projects it guessed. A deleted, dismissed, or resolved item skips the step, as does a workspace with no project.

**What it logs.** Counts, line and message counts, and storage paths, never a title or a word that was said.

### Clips

`ffmpeg` runs in the workspace container (`workspace/Dockerfile`), so the API's image stays small. Each meeting gets a
scratch directory there, `.elysium/meetings/<id>`, holding its captures' recordings (`capture-<capture>.<ext>`) while
its clips are cut, fetched back from storage if the workspace lost them, and removed once the meeting is done. Storage
is where recordings and clips are kept, each clip at `meetings/<id>/segments/<n>.<ext>`. Clips are cut from the
capture's recording on its own clock, as its transcript and Whisper's words are timed.

A segment's times come from the transcript, whose lines a source times to a second or so. When Whisper heard the
recording, its word timings are exact to the audio, so a clip is cut from the start of the word nearest the segment's
start to the end of the word nearest its end (`segmentation::clip_span`), each within three seconds: a word farther away
belongs to speech across a silence Whisper skipped, so that end keeps the transcript's time, as both do for a meeting
Whisper did not transcribe. The record's span stays the transcript's, and its clip names the stretch it holds.

A clip is cut with `-c copy`: fast and lossless, but it starts at the keyframe nearest its start, a second or two early
at most. MP4-family clips are written with `+faststart`, so a browser plays them as they stream.

Recording downloads use their own HTTP client, which follows redirects only to `https` addresses on a named host,
and the first address is held to the same rule, so a source's address cannot lead Elysium into its own network.

## Segment records

Each segment is a timeline record:

| Field          | Holds                                                                          |
|----------------|--------------------------------------------------------------------------------|
| `source_kind`  | `meeting`                                                                      |
| `connector_id` | The capture's connector                                                        |
| `external_id`  | `<the capture's external id>#<segment index>`, or `meeting:<meeting id>#<segment index>` for a capture with none |
| `occurred_at`  | The meeting's start plus the segment's start on the meeting's clock            |
| `actor`        | The segment's speakers, in the order they first spoke                          |
| `title`        | The segment's title                                                            |
| `body`         | Its transcript excerpt, as `[m:ss] Speaker: text` lines                        |
| `url`          | The capture's page at its source                                               |
| `media`        | `{ meetingId, startMs, endMs, clip: { storageLocationId, path, contentType, startMs, endMs } \| null }` on the meeting's clock, the clip's own span cut at Whisper's words |
| `metadata`     | `{ meetingId, meetingTitle, meetingSource, segmentIndex, summary }`, its source the capture's |

Its projects are attributed by `elysia` with her confidence; a person's attribution replaces hers, Elysia never changes
it back, and every change is in the record's history, undoable (`docs/connectors.md`, The timeline).
Each record publishes `timelineRecord.created`, which Elysia's intake hears. `GET /api/v1/timeline/{id}/media`
streams the clip from storage to the record's owner alone, a range at a time (Playing media, below). Each segment is a
document of the knowledge index, stored
with its record: its title, its summary, and its excerpt, never its clip (`docs/knowledge.md`, Timeline records).

## The final transcript

A meeting's final transcript (`meeting_words`, `models::meeting_word`) is a stream of words, each with a stable id, a
speaker, a start and end on the meeting's clock, and where it came from (`origin`): its capture's source transcript
(`source`), its capture's Whisper transcript (`whisper`), a correction of what the capture heard (`correction`), or an
edit (`edit`), with the capture it was heard in (`capture_id`, none for an edit's words). Every word names the change
in the meeting's history that wrote it (`introduced_by`).

- **Built from the one capture.** Until captures are aligned (#238), a meeting's final transcript is built from its
  one capture (`meetings::final_transcript`), from the lines its correction followed: its corrected copy's lines, a
  line that differs from the one it follows being the correction's, or, uncorrected, its source's lines, or Whisper's
  segments when the source sent none. A line becomes its words, split at whitespace, its span spread evenly across
  them (a line timed backwards gives its words no time), each word keeping the line's speaker. It is built once, by
  the system, as a `built` change: by the correction step once its copy is stored, and by segmenting for a meeting
  whose correction was skipped. The migration that split meetings into captures built every corrected or segmented
  meeting's transcript the same way, in SQL, and a test holds the two to the same words.
- **Every word it ever held.** A word is never deleted or rewritten. An edit marks the words of its range replaced
  (`superseded_by`, the change that replaced them) and writes new ones; the transcript is the words that stand, in
  order of `position`. Positions order every word a meeting ever held in one sequence with gaps of 1024, and an edit
  writes its new words just after the last word it replaces, so the words it replaced stay where they were and an undo
  stands them up again between the same neighbours. When no gap is left there, the meeting's words are renumbered in
  one statement, keeping their order.
- **Marks** (`meeting_marks`, `models::meeting_mark`) are rows anchored to a start and an end word: a confidence flag,
  an edit, an excerpt, a black-out, a comment, a pin, or a task (`docs/meeting-review.md`, Marks), each with its data
  and who made it. A range runs between two standing words of the meeting, in order, so it is one word or the whole
  meeting. A removed mark is kept, marked by the change that removed it, so an undo puts it back with whatever hangs
  from it.
- **An edit** (`meeting_word::edit`) replaces a range of standing words with new ones, at most 20,000 characters and
  at least one word. The new words share the replaced words' span, spread evenly, and the first replaced word's
  speaker. Every mark over the old words covers the new ones: one starting in the range starts at the first new word,
  one ending in it ends at the last. The edit makes a mark of its own, of kind `edit`, over its new words, which goes
  with the edit and is never made or removed on its own.
- Every change takes the meeting's lock first, and is recorded in its history in the same transaction.

## History

Every change to a meeting is in its history (`meeting_events`, `models::meeting_event`), shaped like a page's: its
kind, who made it and the agent acting for them, and what it changed. An undo (`meetings::history::undo`) reverses one
change and is a change of its own naming the one it reversed (`undid`), so it can be undone in turn. Only a change
whose effect still stands is undone; one moved on since answers that it changed since, and nothing is overwritten.

| Change                | What it records                                                  | Its undo |
|-----------------------|------------------------------------------------------------------|----------|
| `built`               | The capture its final transcript was built from, from what, and how many words | None: a transcript is built once, and edited after |
| `edited`              | The words it replaced and the words it stood in their place, each mark it moved with its range before and after, the marks it made or removed, and the text before and after | Stands the replaced words up again, moves every mark it moved back where it was unless a later change moved it, moves marks made since over its words onto the words put back, and removes its own mark |
| `marked`, `unmarked`  | The mark, its kind, and its range                                | Removes the mark, or puts it back |
| `hold_changed`        | The hold before and after, by its person holding the meeting or releasing it | Sets it back, while the transcript is not yet corrected, as holding does |
| `correction_approved` | Its person approved the corrected copy                           | None: approving let its run go on |

## Playing media

A capture's recording and a segment's clip stream from their storage location through the API, from Elysium's own
origin, so no provider address or key reaches a browser (`routes::v1::media`). `GET
/api/v1/meetings/{id}/captures/{captureId}/media` plays a capture's recording for its meeting's owner alone, `404` for
anyone else, a capture of another meeting, or one with no recording kept; `GET /api/v1/timeline/{id}/media` plays a
segment's clip for its record's owner.

- **A range at a time.** Both answer a `Range` of one span of bytes with `206 Partial Content` and its `Content-Range`,
  one that covers none of the file with `416`, and anything else with the whole file, each saying `Accept-Ranges:
  bytes`. A player asks for the part it needs, so a two hour recording seeks without being downloaded whole.
- **From the provider, a part alone.** The range goes on to the provider (`Storage::download_range`,
  `docs/storage.md`, Ranges), so the API reads only the part asked for, and nginx is told not to buffer the answer
  (`X-Accel-Buffering: no`), so it reads only as fast as the browser plays rather than a whole recording ahead for
  every seek.
- **What it says it is.** A recording's type is what its fetch stored, or its extension's for one kept without it.
  Only audio and video types are sent as themselves; anything else is sent as bytes, under the `/api` sandbox
  (`docs/security.md`). Each is written once at a path that names it, so a browser keeps what it read, privately.

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
    state (as its run says: failed with why, skipped, or how far it got), that its run waits for the person's
    approval, or that a finished one was published from its transcript alone; when it started, how long it ran, and how
    many topics Elysia found. A failed meeting's row has Retry, which retries its run's failed step at once.
- **A meeting's page** is a page of the knowledge base (`/knowledge/meetings/:meetingId`, `MeetingPage` in
  `src/pages/Knowledge/Meetings/`), designed in `docs/meeting-review.md`, The meeting's page; its old address,
  `/settings/meetings/:meetingId`, lands there with its query. Its header holds its title, source, and state, when it
  started, how long it ran, and a link to it at its source, in a new tab. Two tabs follow, the open one in `?tab=`:
  **Review**, first, and **Topics**, counted: its segments in the order they were said (`MeetingTopics`,
  `GET /timeline?meetingId=`), each shown as on a project's timeline with the projects it is on, which is where a
  topic Elysia put on no project, or the wrong one, is corrected until excerpts take their place.
  - **Layout** (`MeetingWorkspace`). The transcript, three quarters wide, and a lane beside it: the options box, which
    floats at its top as the page scrolls, the mark selected, then the meeting's changes. Below `lg` the lane is a
    drawer, opened by Options above the transcript. The options switch the transcript between Final and each capture's
    own (`CaptureTranscript`, `GET /meetings/{id}/captures/{captureId}/transcript`, as it came, with no marks), turn
    each kind of mark on or off, say what is selected (its span, length, and words) or the moment picked, and hold the
    meeting's actions: where its correction stands, Approve while the copy waits for one, holding it for approval by
    hand until it is corrected unless its source already waits, and its run.
  - **The transcript** (`FinalTranscript`, `GET /meetings/{id}/words`, read through SWR alone) is cut into paragraphs
    at each change of speaker and inside a long turn (`paragraphsOf`), each memoized, so a selection redraws only the
    paragraphs it reaches. Every word and every space between two words is a piece, and each kind of mark draws in its
    own channel (`src/theme/meetingTranscript.css`): confidence as the background from red (0) through yellow (50) to
    green (100), an edit outlined purple inside, a comment outlined just outside (white on the dark theme, the
    foreground's ink on the light one, where white would vanish), a black-out boxed black with its text dimmed, and an
    excerpt as a blue bar in the gutter, one lane per overlap (`excerptLanes`), naming its projects over it. A space is
    marked only when one mark covers the words on both sides of it, so a mark draws as one box. What lies where is
    pure and tested (`meetingTranscript.ts`).
  - **The player** (`MeetingPlayer`, `PlayerFrame`, `useMeetingPlayer`), along the top while the meeting has a
    recording, plays one capture's recording at a time with the browser's own controls, starting on the first that is
    a video, since a picture says who is speaking, and a switcher between them when there are several, which plays on
    from the same moment on the meeting's clock. A recording with audio alone plays over black with its waveform (`PlayerWaveform`),
    drawn live from what is heard, so it needs nothing the player does not already stream. Once the page scrolls past
    its place, which keeps its height so nothing jumps, the same media element floats in the bottom right corner and
    plays on; dragged by its grip it goes anywhere in the window, and stays where it was left, as a share of the room
    the window leaves, for that person on that browser (`usePlayerPlacement`). A meeting in words alone, such as a
    Slack huddle, has no player. Clicking a word seeks to it; once the transcript or the player has been pressed, Space
    plays and pauses, or plays the words selected, and the left and right arrows jump ten seconds; and the word being
    spoken is underlined in the text's own color as it plays, found every frame by a binary search over the words
    (`spokenWordAt`, `meetingPlayer.ts`), so only the paragraphs it moves between redraw.
  - **Clicking and selecting.** A click picks a moment, underlined, which the player seeks; with marks under the
    word, the shortest is selected and every one there shows as a pill in the floating bar, which Tab moves between,
    and the selected one opens in the lane: its kind, who made it (an agent's saying so), its words, and what its kind
    says, such as how sure a confidence is and what else it might be, or what an edit replaced with its Undo. A drag,
    or a click then Shift+click, selects whole words; a touch screen has Select from here, then a tap where it ends.
    What is picked is held by word id, so it stays on its words when the transcript is read again around it. The bar
    offers Edit (E), and Play from here (Space) while the meeting has a recording, and clears with its X or Esc; the
    browser's own text selection is off, and Ctrl+C copies the words selected. Comment, Assign to a project, Black out,
    and Pin a frame join the bar with the issues that build them.
  - **Editing** (`WordRangeEditor`) opens the range in place, its text selected, with a confidence mark's
    `data.alternatives` over exactly that range as one-click choices. Ctrl+Enter or Apply sends it
    (`POST /meetings/{id}/edits`), and the page applies what the route answers (`applyTranscriptChange`) rather than
    reading the transcript again. Words changed meanwhile make no edit and read the transcript again; any other
    refusal keeps the editor open with what was typed.
  - **Every change is an action** (`useMeetingChanges`): it applies at once with Undo in its toast, and there is no
    Save. The page keeps the changes it made that still stand, so Ctrl+Z (Cmd+Z) outside a text field undoes the last
    of them, and further back on each press. The lane's changes (`MeetingHistory`, `GET /meetings/{id}/history`) list
    every change newest first, who made it, and Undo on each that still can be; one moved on since says so and stays.
- **An item asking for a review** links to its meeting's page, from the item's page (`MeetingReviewLink`, reading
  `GET /meetings?reviewItemId=`).
- **The source's setting** is its connector's content form's switch, "Wait for my approval before acting on
  meetings", saved with the page.
- **A project's timeline** (`ProjectTimeline`, the project page's Timeline tab, which counts every record): every
  record attributed to the project, newest first, fifty at a time with "Show older". A meeting segment shows its
  title, its place in the meeting, its summary, its clip (video or audio, loaded only when played), and its
  transcript excerpt on request.
- **What a record is about** (`RecordProjects`, below every record on a project's timeline and every topic): each
  project it is on, Elysia's saying how sure she was. The person confirms hers, which makes it theirs, takes the
  record off a project, or adds it to one from Add to a project; each happens at once, with Undo in its toast. History
  (`RecordHistory`, `GET /timeline/{id}/history`) lists every change, Elysia's and theirs, newest first, each with
  Undo; one changed again since says so and stays.
- Records and meetings reach Redux through SWR loaders and stay current through `timelineRecord.created`,
  `timelineRecord.changed`, and `meeting.changed`, each loading what it names by id, and `workflowRun.changed` for a
  meeting's run, which reloads
  the meeting so its page and row say when the run holds for approval (`docs/frontend.md`). `meeting.changed` also
  reads the open page's transcript and history again, so another tab's edit shows, except on the page that made that
  change, which applied it already (`entryId`, `docs/frontend.md`, Meetings).

## What is published

| Event                       | When                                                                     |
|-----------------------------|--------------------------------------------------------------------------|
| `meeting.changed`           | A meeting was read, its processing moved on, its corrected copy was stored or approved, its hold changed, or a person edited its words or undid a change: its id, state, and owner, and the history entry of an edit or undo (`entryId`), never its title or transcript |
| `timelineRecord.created`    | A segment's record was stored                                            |
| `timelineRecord.changed`    | What a record is attributed to changed: a person's change or undo, or a conversation segmented again |
| `connector.changed`         | A meeting connector was added, a listing changed its sign-in state or error, or a meeting moved it |

## Roadmap

- Meetings as reviewed documents in the knowledge base: captures from several sources and uploads, one final
  transcript with its confidence, edits, excerpts, black-outs, comments, and attachments, public by default. The design
  is `docs/meeting-review.md`, and it covers the Teams and Slack huddle sources.
- Google Meet recordings with their transcripts, read through the person's signed-in browser (`docs/browser.md`):
  they land in Google Drive, reachable as the user, and sign in beside their source in `sources.rs` as Teams' do.
- Teams meetings with a transcript and no recording, whose chat message names no drive item, and recordings of
  meetings whose chats the person's Teams has not stored.
