# Meetings

Elysium reads a person's meetings, their recordings and timestamped transcripts, as that person, and cuts each one
into topic segments. Each segment lands on the timeline (`docs/connectors.md`) as its own record: the recording
trimmed to it, its transcript excerpt, and a summary, attributed to the projects it is about. So when a two hour
meeting spends five minutes on a product, the product's timeline shows that five minute segment.

Meetings come from meeting sources and from uploads. The sources are Fireflies.ai, read with the person's own API key
on a paid plan or through Fireflies' own web app in a browser the person signed in to on any plan, and Microsoft Teams
and Slack's huddles, each read through the browser the person's connector signed in to. A person also uploads their
own recordings and transcripts (Uploads, below). The code is `api/src/meetings/`, and the uploads' rules are
`api/src/meeting_uploads/`.

A meeting is the one event that happened; each of its captures is one record of it, as its source gave it (Meetings
and captures, below). Every capture sits on the meeting's one clock, and their words are lined up into its final
transcript, a stream of words with stable ids, every mark on it a range of those words (Aligning, and The final
transcript, below). One correcting agent settles that transcript, flagging what it is unsure of with a confidence and
alternatives, and the Transcription doc teaches every correction and transcription how the company's speech is misheard
(Correction, and The Transcription doc, below). A meeting recorded twice is merged only when its person says so (The
same meeting twice, below). Every change to a meeting is in its history, and undoable (History, below).
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
  connector read its first capture, or who uploaded it. A meeting whose captures were merged into another names it
  (`merged_into`), holds none, is listed nowhere, takes no upload, and is processed no further until they are split
  back out.
- **A capture** (`meeting_captures`, `models::meeting_capture`) is one record of it: a connector's meeting (a
  Fireflies meeting, a Teams recording, a Slack huddle), or a person's upload (Uploads, below). It keeps what its source
  gave, as it came: its connector, its source and the source's id for it, its page there, the transcript with its
  SHA-256, and the recording with the SHA-256 of its bytes. Both are written once, by its fetch, and never again.
- **One clock.** A capture runs on its own clock, the one its transcript and recording are timed by. Its `offset_ms`
  places that clock on the meeting's: its moment `t` is `t + offset_ms` on the meeting's clock, which starts at the
  meeting's `started_at`. Every time of the final transcript and of a segment's record is on the meeting's clock. How
  each offset was found is its `offset_source` (Aligning, below).
- A listing stores a meeting and its first capture together, in one transaction, so a capture its connector recorded
  before leaves no meeting behind (`meeting::insert_new`).
- **Its captures in order.** A meeting's captures are in the order they joined it (`joined_at`,
  `meeting_capture::list_of`): the one it was listed with first, then each uploaded to it or merged into it. The first
  is the clock every other is aligned against, its corrected copy is the one correction made, and everything shown of a
  meeting's source (its connector, its source, its page there, and whether a recording is kept) is its. The steps that
  fetch, transcribe, and segment read the capture the run names: the meeting's own first one, or the capture an upload
  added, whose run places it on the clock and lines the transcript up again from every capture (Aligning, below).
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
  by the system user (`workflow::run_to_start`). An upload's run is started by its person, as the upload finishes, in
  the transaction that stores its meeting, so the loop never starts one of its own for it (Uploads, below). The same
  pass removes every upload no part reached for a week (`meeting_uploads::STALE_AFTER`), and what of it arrived, and
  the parts of finished uploads that no longer need them (Uploads, below).

| Step                 | Moves the meeting       | What happens                                                    |
|----------------------|-------------------------|-----------------------------------------------------------------|
| `fetch_meeting`      | `pending` to `recorded` | Stores the capture's transcript and uploads its recording, each with its hash (Fetching, below) |
| `transcribe_meeting` | No                      | Transcribes the capture's recording with the local Whisper and stores it beside the source's transcript (Transcription, below) |
| `align_meeting`      | No                      | Places every capture on the meeting's clock, lines their words up into the final transcript, and holds the run while its person says whether it is the same meeting as another (Aligning, below) |
| `correct_transcript` | No                      | Settles the final transcript with confidences and alternatives, stores the corrected copy beside the raw transcripts, teaches the Transcription doc, and holds the run for its person's approval when the source or the meeting asks (Correction, below) |
| `segment_meeting`    | `segmented`, then `done` | Cuts the corrected copy into topics, cuts each one's clip, and publishes every topic as a timeline record attributed to its projects (Segmentation, below) |

Each step's output names the capture it was about (`captureId`), and the step after reads that capture. A step
handed what a run started before meetings had captures gave, which names the meeting alone, reads the meeting's first
capture, logging that it did (`meetings::capture_of`).

A meeting whose captures change after its run aligned it (a merge, or a split) is aligned again by a run of the
`meeting_alignment` workflow, `align_meeting` alone (`workflow::alignment`), started by what changed it. It leaves the
meeting's processing where it was: a meeting segmented already is not segmented again.

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
- **An upload** is fetched from where its parts arrived rather than from a source: the step takes the capture its
  input names (`captureId`), finds the upload it was made from (`meeting_uploads.capture_id`), and stores the lines its
  transcript was read as with their SHA-256, and its recording, joined from its parts as they stream and uploaded to
  the storage location the upload chose at `meetings/<meeting>/captures/<capture>/<sha256>.<ext>`, the hash read from
  the parts first. Once the write is kept, the parts are removed. Parts no longer in the workspace fail the step for
  good, since only the person can send them again; a workspace or a provider that does not answer is tried again. Its
  log names counts, hashes, and paths, never a file's name.
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
- **Told the names first.** Each transcription is given the Transcription doc's entries and the company doc's people
  and glossary as Whisper's hint words (`transcription_doc::hint_words`, The Transcription doc, below), in the
  container's `WHISPER_HINT_WORDS`, which faster-whisper reads as `hotwords`, the start of the model's own context. So
  a name is heard right more often before anything corrects it. A variable rather than an argument, so an engine newer
  than its Whisper image only goes untold, where an argument the image does not know would fail the transcription. The
  log counts them.
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

Transcription mishears names, products, and jargon, and every capture mishears them differently, so one correcting agent
settles each meeting's final transcript against everything it knows before the meeting is acted on.
`correct_transcript` (`api/src/meetings/correct.rs`; the prompt and the checks on the answer are `correction.rs`) takes
what the transcription gives and gives it back with `correctedTranscriptHash`, naming the corrected copy, and
`approved`, whether its person had approved it when the run went on, as segmenting's input
(`TranscriptToSegment::Meeting`, whose JSON is `CorrectedMeeting`'s own). What the transcription gives reads as that
with no copy, so a person may skip the step and the next one still reads its input. A meeting not fetched skips it,
saying "Not fetched, so nothing to correct". The raw transcripts are never written.

- **What it reads.** The meeting's whole final transcript, lined up from every capture (and lined up here first when a
  person skipped aligning), as numbered lines of one speaker each, broken at a pause of two seconds and every sixty
  words. Under each line, every capture's own transcript of the same moment that heard it otherwise, its source's lines
  and its Whisper's alike, on the meeting's clock (`correction::Transcript`); a source transcript with no times at all,
  such as one uploaded beside its recording, has no moment to set beside a line and is left out, its words already the
  final transcript's when Whisper heard nothing. Agreement is not trust: a name every capture mishears the same way is
  still wrong, and Whisper is one capture among the others, never the source of truth. Beside it, the Transcription doc
  whole (The Transcription doc, below); the company doc whole, with the suggestions pending on its sections
  (`company_doc::read`, `docs/knowledge.md`), its sections in the order correction needs them (people, glossary,
  products, projects, stakeholders, leadership, company), the lines a pending suggestion would add marked as proposed;
  and the names speakers went by in the 30 newest meetings' captures, as their sources labelled them, which is usually
  how people spell themselves. Other meetings are anyone's in the company: the names and terms a company uses are its
  own, as its knowledge base is (`docs/knowledge.md`, Who sees what). A meeting whose first capture heard nothing, or
  with no words, skips the step, saying "Nothing was said, so nothing to correct".
- **The turn.** One text turn on the runner, however long the meeting, on the model resolved for the step, of the
  correction kind: Opus 5.5 at high effort unless an admin chose another for the step or the kind (`docs/engine.md`,
  Models). Its prompt holds the meeting's title and participants, both docs and the speakers' names, once, then every
  line with what each capture heard otherwise. The runner hands the CLI its prompt on standard input (`docs/coding.md`,
  How a prompt reaches the CLI), so nothing is cut to fit. A meeting too long for the model's context fails the step
  for good, giving the CLI's reason, since every attempt would read the same prompt (`TurnOutcome::prompt_too_long`);
  skipping the step on the Runs page lets segmenting go on from the transcript as heard.
- **The answer** is `{ "flags": [{ "line", "heard", "text", "confidence", "alternatives", "why" }] }`: each flag names a
  run of a line's words by copying them, the text it decided on, its confidence from 0 to 100, up to three other things
  the run may have said, and why, in a sentence. A run it cannot make out is flagged at 0 with no alternatives, for a
  person to listen to.
- **Nothing in the answer is trusted.** A flag is kept only for a line the transcript has, over words found in that
  line exactly once (compared without regard to case or the punctuation around a word), overlapping no flag kept before
  it, and its text replaces only those words. Its text must only fix words: each change replaces at most four words
  with at most four (a name heard as several words is one fix), none adds words where there were none or drops words
  without a replacement, and the words replaced are at most half the run's (or four, on a short run). Its confidence is
  rounded and clamped to 0 to 100; a flag that changes words without one is held at 50, and one that changes nothing
  and says nothing is dropped. Its alternatives are kept when they are text, each once, none the text it decided on,
  each only fixing words as its text must, and at most three. A refused flag leaves its words as they were, and is
  logged by its line number and why. An answer with no JSON, or the wrong shape, fails the attempt, which the run's
  backoff tries again.
- **Settling.** In one transaction under the meeting's lock (`meeting_word::settle`): each flag whose text differs from
  its words replaces them with words of the correction's own (origin `correction`, their capture the words'), sharing
  their span, every mark over the old words covering the new ones; and every flag makes a confidence mark over its
  words, `{ confidence, alternatives }`. A run whose words differ from what a capture heard over the same line, with no
  flag over it and no person's edit in it, is marked changed at 50, its alternatives what the captures heard there
  (`correction::unflagged_changes`), so nothing the transcript changed from a capture goes unseen. A run whose words
  changed since the turn read them, by a person's edit meanwhile, is passed over. It is one `settled` change in the
  meeting's history, by the system (History, below).
- **The copy, and what it teaches.** In the same transaction, the corrected copy of its first capture's lines is
  stored, each flag that lies inside one of those lines in place (The corrected copy, below), and the Transcription doc
  learns each fix of every flag at 80 or more (The Transcription doc, below).
- **Once.** The copy is stored in `meeting_corrections` (`docs/database.md`), once: an attempt that finds it answers
  it without a turn, and one that finds another attempt stored it first discards its own. A rebuild keeps what a
  settlement wrote (The final transcript, below), so a hand-set offset, a merge, or a split lines the transcript up
  again and never settles it again. A meeting merged into another skips the step, saying "Merged into another meeting,
  so nothing to correct".
- **What it logs.** Word, line, and flag counts, line numbers, and hashes, never a word that was said.

#### The corrected copy

What the steps after correction read, segmenting first, until segmenting reads the final transcript's words
(`docs/meeting-review.md`, Excerpts). The step's output names it by its hash, and `correct::corrected_lines` reads the
copy an output names.

| Field            | Holds                                                                                     |
|------------------|-------------------------------------------------------------------------------------------|
| `transcript`     | An array of `{ startMs, endMs, speaker, text }`, the shape of `meeting_captures.transcript`: one per line of the first capture's transcript it follows, in its order, with its speaker and times; only `text` differs, each flag that lies inside the line in place, so a flag over another capture's words or across two lines is in the final transcript alone |
| `transcript_hash`| The SHA-256 of `transcript` as stored, which the output's `correctedTranscriptHash` names  |
| `based_on`       | `source` or `whisper`: the transcript it follows                                          |
| `changed_lines`  | How many lines it changed                                                                 |
| `fixes`          | Each distinct fix its flags made, `{ heard, written }`                                   |
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

**A meeting in words alone**, a transcript uploaded with no times and no recording, has no clock to answer in: every
line is at `0` (`meeting_uploads::transcript::is_untimed`). One uploaded with a recording is read on Whisper's clock
instead (Correction, above). Its prompt numbers the lines, with no times, and its answer
names each segment by its first and last line (`segmentation::parse_lines_answer`), kept as a conversation's runs are.
Each segment keeps its run of lines (`Segment.lines`), which its excerpt and speakers read, and carries no clip; its
record's span is `0` to `0`, which the web app reads as no place in the meeting.

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
   meeting whose aligning was skipped has its final transcript lined up here (The final transcript, below). A meeting
   in which nothing was said is `done` with no topics, and the step skips ("Nothing was said, so nothing to segment"),
   and one merged into another skips it ("Merged into another meeting, so nothing to segment").
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

## Aligning

`align_meeting` (`api/src/meetings/align.rs`; the arithmetic is `alignment.rs`) takes what the transcription gives and
gives it back, so a person may skip it. A meeting not fetched skips it ("Not fetched, so nothing to align"), and one
merged into another skips it ("Merged into another meeting, so nothing to align").

- **Placing each capture.** The first capture is the meeting's clock. Every other capture whose offset is still the one
  its listing gave (`offset_source` `listed`) is aligned against it:
  - **By audio**, when both have a recording Whisper heard: `ffmpeg` in the workspace writes each recording's energy
    envelope, the mean square of its audio over every ten milliseconds (two hours is 720,000 values), and the two are
    cross-correlated by the fast Fourier transform. The lag wins when the two share at least a minute of audio, its
    correlation reaches 0.3, and it stands 0.1 above the best lag more than two seconds from it, so a silence or hold
    music never decides by a hair. Its offset is the first capture's plus the lag (`audio`).
  - **By words**, otherwise, or when the audio gives no clear lag: every word of four letters or more that each of the
    two says at most three times votes for the gap between when the two heard it, and the gap 1.5 seconds wide with the
    most votes wins, given at least five and twice the votes of any gap more than five seconds from it. Its offset is
    the median vote (`words`).
  - Neither answering, the capture keeps its listed offset, and the log says so. An offset found, or set by a person, is
    never looked for again; a new search happens only for a capture merged in.
- **Lining the transcript up.** Then the final transcript is lined up from every capture (The final transcript, below).
- **The same meeting twice.** Last, the meeting may ask its person whether it is the same as another (The same meeting
  twice, below), and the run waits here until they answer (`Done::Held`).
- **Set by hand.** A person sets a capture's offset (`PUT /api/v1/meetings/{id}/captures/{captureId}/offset`,
  `meetings::clock`), at most twelve hours either way: it is `hand`, recorded in the meeting's history as
  `offset_changed`, the transcript is lined up again at once, and aligning never moves it after. Its undo, from the
  history on the meeting's page, puts the offset back while nothing moved the capture since. The page's controls for
  setting an offset and splitting a capture out are on the roadmap, among the meeting's options
  (`docs/meeting-review.md`, The meeting's page).
- **What it logs.** Counts, offsets, and how each was found, never a title or a word that was said.

## The same meeting twice

Teams and Fireflies often record the same meeting, and each listing stores a meeting of its own. Elysium never merges
them on its own (`api/src/meetings/duplicates.rs`).

- **Asking.** As it is aligned, a meeting looks for another of its person's that overlaps it in time (or, when either
  source kept no length, started within ten minutes of it) with a participant in common. Sources name people differently
  (Fireflies by address, Teams and Slack by name), so a participant is compared, without regard to case, by what the
  source says and by the forms an address is commonly made from a name: `Ada Lovelace` is `adalovelace` and
  `alovelace`, as `ada.lovelace@` and `alovelace@` are, and a one-word name is the address `name@`. The
  later of the two to be listed asks: an inbox item for its person, "Same meeting as" the other's title, raised as the
  system, and a pair in `meeting_duplicates` recording it once, whichever meeting found the other. Its run waits at
  aligning for the answer, so a duplicate never publishes segments of its own. Items are the whole workspace's, so it
  carries the two titles, and nothing that was said.
- **Only the person's own meetings** are compared, since a meeting is its owner's alone until meetings are public by
  default (`docs/meeting-review.md`, Who sees a meeting); comparing everyone's meetings comes with that.
- **Merge** (`POST /api/v1/meetings/duplicates/{id}/merge`) moves the meeting's captures into the other, each placed
  on its clock from when the two meetings started, after its own captures. Every edit made on the merged meeting is
  carried across with its words, the words it replaced, and its mark, the other's transcript is lined up again keeping
  every edit, and a `meeting_alignment` run starts, which finds each moved capture's offset. The merged meeting names
  the one it went into. Both histories record `merged`, with where each capture sat before and after.
- **Not the same** (`POST .../not-the-same`) is remembered, so the pair is never asked about again.
- Either resolves the item and lets the waiting run go on; a merged meeting's steps then skip it. Only the meeting's
  person answers; a pair decided answers `409 duplicate_decided`, and of two answers at once the first to be written
  counts, the other answered that way.
- **Splitting.** A merge is undone by splitting the captures back out (`POST
  /api/v1/meetings/{id}/captures/{captureId}/split`, or undoing the `merged` entry), while each capture still sits as
  the merge left it, or on an offset aligning found since; one a person set by hand since answers that it changed
  since, as undoing would overwrite it. Each goes back where it sat, the
  edits the merge carried are taken back, the merged meeting is its own again, both transcripts are lined up again,
  the pair becomes not the same, and runs start that align the kept meeting again and carry the split one on from
  aligning (or align it alone, if it was segmented). Both histories record `split`, and undoing it merges them again.
- **On the item's page** (`SameMeetingPanel`, reading `GET /api/v1/meetings/duplicates?itemId=`): both meetings,
  each linking to its page, with Merge and Not the same until the person answers, and what they answered after.

## Uploads

A person uploads their own meetings: an OBS recording, an audio file, an old transcript. An upload is a capture like any
other, transcribed, corrected, and segmented by the meeting's run. The rules are `api/src/meeting_uploads/`, the rows
`models::meeting_upload`, and the routes `/api/v1/meeting-uploads` (`docs/api.md`).

- **What a meeting takes.** A recording as `.mp3`, `.wav`, `.mp4`, or `.mov`, up to 5 GiB (what storage takes in one
  upload), a transcript as WebVTT, SRT, plain text, or Markdown, up to 10 MB, or one of each, which is one capture. What
  a file is comes from its extension; nothing the browser says about it is believed. Its name is cleaned as a page
  attachment's is (`crate::file_names`). The limits and the extensions are constants, served at
  `GET /api/v1/meeting-uploads/policy`, so the web app refuses a file before it is sent, and the API refuses again.
- **A new meeting or a capture.** A new meeting names its title and when it started; a capture names a meeting its
  person has, whose run must be done, since a meeting holds one run at a time: `409 meeting_processing` while it is
  going or held, and `409 meeting_run_failed` while it failed, until its person retries or skips it.
- **Where the recording goes.** A storage location the person chooses, offered first as the one the owner already keeps
  meetings in: the meeting's first recording's for a capture, otherwise their oldest meeting source's. A transcript
  alone keeps nothing in storage.
- **Parts.** Every file arrives in parts of 8 MiB (`PART_BYTES`), the last one shorter, each its own request, in order
  (`PUT .../{kind}/parts/{index}`). Each streams into the workspace, `.elysium/uploads/<upload>/<kind>/<part>`
  (`meeting_uploads::staging`), where the engine reaches it too. Each send is written under a name of its own, and
  one that fails takes what it wrote with it. A whole send is put under the part's name and counted under the lock on
  its file's row, so of two sends of one part at once the one counted is the one kept and the other is let go. A part
  already received is answered as it is; one past the next is refused, saying how far the file got. Nothing holds
  more than a chunk of a part in memory.
- **What is checked as it arrives.** A recording's first part must open as its format does (an MP3's ID3 tag or frame
  sync, WAV's `RIFF ... WAVE`, an MP4's or QuickTime's opening box), since it is later played on Elysium's origin. A
  transcript's last part is counted only once the whole transcript reads: UTF-8 or UTF-16 text, at most 100,000 lines,
  with something said in it. The web app sends the transcript first, so one that cannot be read is refused before a
  recording is sent after it.
- **Resuming.** An upload keeps how much of each file arrived, and its files' names, sizes, and when they last changed.
  The upload page reads the person's unfinished uploads, and choosing the same files for the same meeting again carries
  on from the first part the API lacks, after a dropped connection, a reload, or Upload pressed again once an upload
  stopped. A part that fails on the way is sent again, waiting longer each time, up to eight times. An upload no part
  reached for a week is removed with its parts.
- **Parts are removed once.** The fetch removes an upload's parts once it stored them. Whatever it could not remove, and
  the parts of an upload finished a week ago whose capture was never stored and whose meeting's run settled (its fetch
  skipped by a person), the meetings loop's sweep removes, recording it (`meeting_uploads.parts_removed_at`) so it
  happens once. A fetch that failed keeps its parts however long it waits for a person to retry it; a fetch that finds
  them removed fails for good, asking for the recording again.
- **Finishing.** Once every part arrived, finishing stores the meeting with its capture (or the capture of its meeting),
  marks the upload finished with the lines its transcript was read as, and starts the meeting's run from its fetch, as
  its person, all in one transaction; the fetch then stores the recording (Fetching, above). A new meeting's duration is
  where its transcript's last line ends, when it has times, and its participants are the speakers its transcript names.
  Finishing again, or twice at once, answers the same meeting.

### Reading a transcript

`meeting_uploads::transcript` reads an uploaded transcript into the lines every source's transcript is stored as.

- **WebVTT and SRT** are cues: each cue's times, then what was said. A WebVTT voice tag (`<v Ada Lovelace>`, as Teams
  writes them) names the speaker, or else a leading `Speaker:` (as Zoom writes them); every other tag and WebVTT's
  entities are read past, and comments, styles, and regions are skipped.
- **Plain text and Markdown** are one line said per line. `[00:01:23]`, `(1:23)`, or a bare `00:01:23` at a line's
  start times it, and `Speaker:` names who said it; a line with no speaker of its own was said by the last one named.
  A line that is only a speaker, a time, or both (`Ada Lovelace (00:01:23)`) heads the lines that follow. Markdown's
  headings, lists, quotes, and emphasis are read past, and a heading that names nobody is a title.
- A line with no time of its own starts when the line before it did, and each ends when a later line starts later. A
  transcript with no time anywhere is **a meeting in words alone**: every line at `0`, reviewed, edited, and segmented
  by its lines (Segmentation, above), with no player.

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
streams the clip from storage to the record's owner alone. Each segment is a document of the knowledge index, stored
with its record: its title, its summary, and its excerpt, never its clip (`docs/knowledge.md`, Timeline records).

## The final transcript

A meeting's final transcript (`meeting_words`, `models::meeting_word`) is a stream of words, each with a stable id, a
speaker, a start and end on the meeting's clock, and where it came from (`origin`): its capture's source transcript
(`source`), its capture's Whisper transcript (`whisper`), a correction of what the capture heard (`correction`), or an
edit (`edit`), with the capture it was heard in (`capture_id`, none for an edit's words). Every word names the change
in the meeting's history that wrote it (`introduced_by`).

- **Lined up from every capture.** A meeting's final transcript is lined up from its captures
  (`meetings::final_transcript::rebuild`, the arithmetic in `meetings::alignment`), each on the meeting's clock. Each
  capture's words come from its lines: its source's, or Whisper's segments when the source sent none. Each word of a line takes Whisper's timing when Whisper heard the same word within two seconds of
  the line, matched in order, since Whisper's timings are exact to the audio; the rest are spread evenly across the
  time left between the words Whisper anchored, or across the whole line (a line timed backwards gives its words no
  time). A word keeps its line's speaker, and one Whisper alone heard takes the speaker of a source's line at that
  moment, since Whisper tells nobody apart. Then the captures are laid out by time: every word of the first capture,
  and of each later one only the words where no capture before it heard anything within two seconds, so a moment two
  captures heard reads once and a stretch only one caught is still there. A capture Whisper never heard is built
  exactly as the migration that split meetings into captures built every uncorrected transcript, in SQL, and a test
  holds the two to the same words.
- **Lined up again.** It is lined up by the align step, again by everything that moves a capture (a hand-set offset,
  a merge, a split), and by correction and segmenting for a meeting whose aligning was skipped, by the system or by the
  person whose change caused it, as a `built` change. Each compares each capture's words as it gives them now with its
  words that stand, in order, by what each says (its text, speaker, and origin) and not by when, and changes only what
  differs: a word that says the same keeps its id, so marks over it never move, and takes its new times, so a capture
  placed anew moves its words rather than replacing them. One capture's comparison that runs past two seconds settles
  for a larger diff. Every edit's words stand through it, and so do the words the meeting's correction settled while
  their capture is the meeting's, a settlement standing as an edit does (a settled word of a capture split back out
  goes with its capture); the captures' words inside an edit's time stay out. An edit belongs to the capture of the
  words it replaced, and it and they move with that capture, by as much as its offset moved since its words were last
  placed (the newest `built` entry naming it, or the merge that brought it), so an edit stays over what it replaced
  wherever its capture goes on the clock. A standing word another capture heard under an edit is replaced too, even
  when it says the same, but only once the edit's capture is placed: an edit over a capture still at its listed offset
  sits at a time nobody has checked. A merge carries a person's edits into the meeting the captures go into, never a
  settlement. A mark over a word a rebuild replaced moves onto the new words at the same moment. A rebuild is not
  undone on its own; what caused it is, and that lines the transcript up again.
- **Every word it ever held.** A word is never deleted, and what it says (its text, speaker, and origin) is never
  rewritten; only its times move, with its capture's place on the clock. An edit marks the words of its range replaced
  (`superseded_by`, the change that replaced them) and writes new ones; the transcript is the words that stand, in
  order of `position`. Positions order every word a meeting ever held in one sequence with gaps of 1024, and an edit
  writes its new words just after the last word it replaces, so the words it replaced stay where they were and an undo
  stands them up again between the same neighbours. When no gap is left there, the meeting's words are renumbered in
  one statement, keeping their order.
- **Marks** (`meeting_marks`, `models::meeting_mark`) are rows anchored to a start and an end word: a confidence flag,
  an edit, an excerpt, a black-out, a comment, a pin, or a task (`docs/meeting-review.md`, Marks), each with its data
  and who made it. A confidence mark's data is `{ confidence, alternatives }`: its correction's flag, or 50 with what
  the captures heard for a run the transcript changed from one of them (Correction, above). A range runs between two standing words of the meeting, in order, so it is one word or the whole
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
| `built`               | The captures it was lined up from with their offsets, what the first capture's words were lined up from (`source` or `whisper`), and how many words stand, were written, were replaced, and were kept at new times, and how many marks moved | None: what caused it is undone, and lines it up again |
| `settled`             | Each range its correction changed, as an edit records its own, every confidence mark it made, and how many ranges it passed over because their words changed since it read them | None: a person edits what it settled, its alternatives offered on the meeting's page |
| `edited`              | The words it replaced and the words it stood in their place, each mark it moved with its range before and after, the marks it made or removed, and the text before and after | Stands the replaced words up again, moves every mark it moved back where it was unless a later change moved it, moves marks made since over its words onto the words put back, and removes its own mark |
| `marked`, `unmarked`  | The mark, its kind, and its range                                | Removes the mark, or puts it back |
| `hold_changed`        | The hold before and after, by its person holding the meeting or releasing it | Sets it back, while the transcript is not yet corrected, as holding does |
| `correction_approved` | Its person approved the corrected copy                           | None: approving let its run go on |
| `offset_changed`      | A capture's offset and how it was found, before and after, set by hand | Puts the offset back, while nothing moved the capture since |
| `merged`              | In both meetings: which meeting's captures went into which, and where each sat before and after | From the kept meeting's: splits them back out |
| `split`               | In both meetings: the same, as a merge's undo                   | From the kept meeting's: merges them again |
| `capture_added`       | Its person uploaded a capture of it: the capture, its source, and whether it holds a recording and a transcript | None: the capture is the meeting's once its run takes it in |

## The Transcription doc

One fixed page of the knowledge base, Transcription (`crate::transcription_doc`, at an id fixed in code, beside Learnings
and the Company page), is the one source of truth for how the company's speech is misheard and what it is, such as
"Sheriff" heard for "Sharath" in Teams. It is a page like any other: people edit it directly, every change a version
(`docs/knowledge.md`, Pages), and it is indexed by its text, as every page Elysium seeds is.

- **Its entries** are a table, `| Heard | Is | Sources | Speakers | Meetings |`: what was heard, what it is, the
  transcripts that heard it so (a source such as Teams, or Whisper), who was speaking, and links to the ten newest
  meetings it was heard in. Below it, **What it taught itself** lists each entry Elysium added or heard again, newest
  first, each with when, where, and why, for a person to review and prune. The hundred newest stay on the page; every
  older one is in its versions.
- **It teaches itself, live at once.** A person's edit on a meeting's page that only fixes words, as a correction's text
  must (`meetings::fixes::edit`), and each fix of a flag the correcting agent was sure of, at 80 or more
  (`correct::TEACHING_CONFIDENCE`), add their entry or strengthen it with their source, speaker, and meeting, in the
  transaction that made them, as one version of the page by whoever taught it: the person, or the system for a
  correction. An edit that rewrites more than that, or only changes case and punctuation, teaches nothing, and undoing
  an edit leaves what it taught.
- **People's edits stay.** Elysium reads and writes only the table and the list, found by their header and heading,
  and edits their lines in place, so a person's own rows, notes, and order stay as they wrote them. A row it cannot
  read is passed over, and a table or heading a person removed comes back at the page's end the next time it learns.
- **Every correction reads it** whole in its prompt (Correction, above), and **every transcription** is told what its
  entries are, those heard in the most meetings first, then the names and terms the company doc's People and Glossary
  sections spell (each line's text before its first colon, dash, bracket, or comma), each once, within 500 characters
  (Transcription, above).
- **The company doc.** A lesson naming someone or something the company doc's People and Glossary do not, nor any
  suggestion waiting on them, is suggested to that section's page as one line, `- Sharath: often heard as "Sheriff"`:
  to People when it names one of the meeting's participants or speakers, and to the Glossary otherwise, for a person
  to accept or dismiss there (`docs/knowledge.md`, The company doc, and Pages, Suggestions). A correction suggests as
  Elysia, naming its run, and a person's fix as them.

## In the web app

- **Settings, Meetings** (`/settings/meetings`, in the directory's Knowledge base section, `MeetingsPage`): the
  person's recent meetings and their meeting sources, as two tabs with their counts, the open one in `?tab=`, with
  Upload a meeting in the titlebar. The page
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
    hand until it is corrected unless its source already waits, its run, and Add a recording or transcript (Uploading,
    below).
  - **The transcript** (`FinalTranscript`, `GET /meetings/{id}/words`, read through SWR alone) is cut into paragraphs
    at each change of speaker and inside a long turn (`paragraphsOf`), each memoized, so a selection redraws only the
    paragraphs it reaches. Every word and every space between two words is a piece, and each kind of mark draws in its
    own channel (`src/theme/meetingTranscript.css`): confidence as the background from red (0) through yellow (50) to
    green (100), an edit outlined purple inside, a comment outlined just outside (white on the dark theme, the
    foreground's ink on the light one, where white would vanish), a black-out boxed black with its text dimmed, and an
    excerpt as a blue bar in the gutter, one lane per overlap (`excerptLanes`), naming its projects over it. A space is
    marked only when one mark covers the words on both sides of it, so a mark draws as one box. What lies where is
    pure and tested (`meetingTranscript.ts`).
  - **Clicking and selecting.** A click picks a moment, underlined, which the player will seek; with marks under the
    word, the shortest is selected and every one there shows as a pill in the floating bar, which Tab moves between,
    and the selected one opens in the lane: its kind, who made it (an agent's saying so), its words, and what its kind
    says, such as how sure a confidence is and what else it might be, or what an edit replaced with its Undo. A drag,
    or a click then Shift+click, selects whole words; a touch screen has Select from here, then a tap where it ends.
    What is picked is held by word id, so it stays on its words when the transcript is read again around it. The bar
    offers Edit (E) and clears with its X or Esc; the browser's own text selection is off, and Ctrl+C copies the words
    selected. Comment, Assign to a project, Black out, Pin a frame, and Play from here join the bar with the issues that
    build them.
  - **Editing** (`WordRangeEditor`) opens the range in place, its text selected, with a confidence mark's
    `data.alternatives` over exactly that range as one-click choices. Ctrl+Enter or Apply sends it
    (`POST /meetings/{id}/edits`), and the page applies what the route answers (`applyTranscriptChange`) rather than
    reading the transcript again. Words changed meanwhile make no edit and read the transcript again; any other
    refusal keeps the editor open with what was typed.
  - **Every change is an action** (`useMeetingChanges`): it applies at once with Undo in its toast, and there is no
    Save. The page keeps the changes it made that still stand, so Ctrl+Z (Cmd+Z) outside a text field undoes the last
    of them, and further back on each press. The lane's changes (`MeetingHistory`, `GET /meetings/{id}/history`) list
    every change newest first, who made it, and Undo on each that still can be; one moved on since says so and stays.
- **Uploading** (`src/pages/MeetingUploads/`): Upload a meeting opens `/settings/meetings/new`, and the actions in a
  meeting's options hold Add a recording or transcript, `/settings/meetings/:meetingId/captures/new`, the same form for
  a capture. A recording and a transcript field each take a file from the picker or dropped on it, refusing a type or a
  size the policy does not take in their own words; a new meeting's title and when it started follow its first file (its
  name, and when it last changed) until the person changes them, and a recording asks where it is kept. Upload, the
  page's one save, starts the upload or resumes the one the same files began (`findResumable`), sends its parts with the
  progress beside the fields (`sendMeetingFiles`), and once it is whole opens the meeting. A transcript the API cannot
  read is let go and marked on its field. Leaving while it runs asks first, and stops it where it is, for the same files
  to resume. The source of an uploaded meeting reads Uploaded (`MeetingSourceLogo`, `meetingSourceNameKey`).
- **An item asking for a review** links to its meeting's page, from the item's page (`MeetingReviewLink`, reading
  `GET /meetings?reviewItemId=`).
- **An item asking whether two meetings are one** shows both, each linking to its page, with Merge and Not the same
  (`SameMeetingPanel`, The same meeting twice, above).
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
| `meeting.changed`           | A meeting was read, its processing moved on, its transcript was settled and its corrected copy stored, or the copy approved, its hold changed, a capture's offset was set by hand, it was merged or split, or a person edited its words or undid a change: its id, state, and owner, and the history entry of an edit or undo (`entryId`), never its title or transcript |
| `page.changed`              | The Transcription doc learned from a correction or a person's fix, or a name was suggested to a company doc section: the page's id alone |
| `timelineRecord.created`    | A segment's record was stored                                            |
| `timelineRecord.changed`    | What a record is attributed to changed: a person's change or undo, or a conversation segmented again |
| `connector.changed`         | A meeting connector was added, a listing changed its sign-in state or error, or a meeting moved it |

## Roadmap

- Meetings as reviewed documents in the knowledge base: captures from several sources aligned onto one clock, one final
  transcript with its confidence, edits, excerpts, black-outs, comments, and attachments, public by default. The design
  is `docs/meeting-review.md`, and it covers the Teams and Slack huddle sources.
- Google Meet recordings with their transcripts, read through the person's signed-in browser (`docs/browser.md`):
  they land in Google Drive, reachable as the user, and sign in beside their source in `sources.rs` as Teams' do.
- Teams meetings with a transcript and no recording, whose chat message names no drive item, and recordings of
  meetings whose chats the person's Teams has not stored.
- Seeking inside a clip: the media route serves a clip whole and does not answer range requests yet.
