# Meeting review

A meeting is a living document in the knowledge base. Its final transcript is built from every capture of it
(Fireflies, Teams, a Slack huddle, a file someone uploaded, and Elysium's own Whisper), and shows where it is unsure,
what it changed, what each part was taken into, and what was said about it. Anyone can correct it by ear against the
recording, and what is relevant, and only that, flows on to projects, their Project Managers, and workers.

This doc is the design. Built so far: meetings and their captures, one clock with each capture's offset, Whisper per
capture, the final transcript as words with stable ids, marks anchored to ranges of them, edits, and every change in a
meeting's history, undoable; aligning every capture onto the clock and lining their words up into the final
transcript, and asking before merging a meeting recorded twice, with merging and splitting; the meeting's page in the
knowledge base, with its layout, every kind of mark drawn, clicking, selecting, and editing in place, each change
undoable, its player, and comment threads on ranges, with mentions, resolving, and Elysia answering in them, open to
its owner alone until meetings are public by default; uploads of recordings and transcripts as new meetings or
captures of one; and meetings in the knowledge base's tree, each at home under the Meetings page and in the Meetings
group of every project's brief it is about, with the Meetings page listing, searching, and filtering them and
starting uploads, Settings keeping only their sources. What is built is described in `docs/meetings.md`, which this
design changes where it says so; the rest is the roadmap below, as issues in the Knowledge base milestone.

## Meetings and captures

- A **meeting** is the one event that happened. A **capture** is one record of it: a connector's meeting (Fireflies,
  a Teams recording, a Slack huddle) or an upload. A meeting has one or more captures, and every capture belongs to
  one meeting. `UNIQUE (connector_id, external_id)` is the captures'.
- A capture holds what its source gave, kept as it came and never written: its transcript, its media when there is
  any, and the hashes of both. Every capture with audio is transcribed by Whisper on its own, so a meeting with a
  Teams and a Fireflies recording has two Whisper transcripts.
- **One clock.** Every capture has an offset onto the meeting's clock. Captures with audio are aligned by
  cross-correlating their audio against the first capture's; a transcript-only capture is aligned by matching its words
  and times against the meeting's transcript. A person nudges an offset by hand from the meeting's options, recorded and
  undoable. Every time in this doc is on the meeting's clock.

### The same meeting twice

Teams and Fireflies often record the same meeting, and two people's Fireflies may each record it. Elysium never merges
captures on its own. A capture whose time overlaps another meeting's, with participants in common, raises an action
item for its owner: "Same meeting as" the other's title, with Merge and Not the same, and its meeting's run waits at
aligning for the answer, so a duplicate never publishes segments of its own. Merging moves its captures into the other
meeting and builds the final transcript again; Not the same is remembered, so the pair is never raised twice. A merge
is undone by splitting the capture back out, and both are in the meeting's history. Until meetings are public by
default (Who sees a meeting, below), only a person's own meetings are compared.

## Who sees a meeting

> "Everyone benefits from all knowledge, nothing is secret by default."
>
> Alex Navarro, 2026-11-09

- **Public by default.** Every member reads every meeting: its transcripts, its media, its comments, and its
  attachments. Guests read only a meeting shared with them. Today meetings are their owner's alone; this replaces that.
- **Private, shared with some.** A private meeting is read by its owner and the people it is shared with alone. The
  owner (or an admin) makes a meeting private, shares it, or opens it again, each recorded.
- **Private until checked.** A new meeting is private to its owner, and indexed for nobody else, until its correction
  has read it. If the correcting agent finds it sensitive (a one-to-one with a manager, a performance review, anything
  personal), the meeting stays private, shared with the Elysium users it judges should see it, such as the people in
  it, and the agent raises an action item for the owner saying what it decided and why, to change if they disagree.
  Otherwise it opens to everyone. Either acts at once.
- **Search follows it.** The knowledge index gains a visibility filter for meetings: a private meeting's passages are
  found only by the people who can read it. Every other source stays as it is (`docs/knowledge.md`, Who sees what).
- **Excerpts still flow.** An excerpt of a private meeting (Excerpts, below) goes to its projects as any excerpt does,
  since it is on their topic; the rest of the meeting stays private.
- **Who changes it.** Any member who can read a meeting edits it: its words, its excerpts, its black-outs, its pins, and
  its attachments. Every change is in its history with who made it, and undoable. Only its owner and admins change
  its privacy, merge or split its captures, or delete it.

## The final transcript

The final transcript is a stream of words, each with a stable id, a speaker, a start and end on the meeting's clock,
and where it came from. Everything on the page (a confidence, an edit, an excerpt, a black-out, a comment, a pin, a
task) is anchored to a range of word ids, so a range can be one word or the whole meeting.

- **Aligning.** Every capture's words are lined up by time. Whisper's word timings are exact to the audio and anchor
  the alignment; the sources' speakers name who spoke, since Whisper tells nobody apart. Where every capture says the
  same, the word stands.
- **Settling.** One correcting agent reads the whole aligned transcript, every capture's version side by side, against
  the Transcription doc (below) and the company doc. Agreement is not trust: a name every source mishears the same way
  is still wrong, and some speech no source heard well. It answers with flags, each over a range of words: the text it
  decided on, its confidence from 0 to 100, and up to three alternatives. It may flag a range at 0 with no
  alternative, meaning it cannot tell what was said; that range is for a person to listen to. Whisper is one capture
  among the others, never the source of truth.
- **What changed shows itself.** A range whose final text differs from a capture's, with no flag from the agent, is
  marked changed at a medium confidence, 50, so nothing the transcript changed goes unseen.
- **Nothing in the answer is trusted.** A flag is kept only over word ids it was given, without overlapping another, and
  its text replaces only that range; every word, speaker, and time outside a flag stays as aligned. The checks today's
  correction makes (`docs/meetings.md`, Correction) hold per range.
- **Whisper is told the names.** Each transcription is given the Transcription doc's written forms and the company
  doc's people and glossary as Whisper's hint words, so a name is heard right more often before anything corrects it.
- **One prompt, however long.** The runner hands the CLI its prompt on standard input (`docs/coding.md`, How a prompt
  reaches the CLI), so the whole aligned transcript, every capture side by side, is one turn's prompt, with the
  Transcription doc and the company doc read once.

### The Transcription doc

One fixed page in the knowledge base, Transcription, beside Learnings and the Company page: how speech is misheard and
what it should say, such as "Sheriff" heard for "Sharath" in Slack and Teams. One doc is the one source of truth for
it, and it replaces what corrections learn from the 30 newest today.

- Each entry is what was heard, what it is, where it is heard (a source, a speaker), and the meetings it came from.
- **It teaches itself, and every entry is live at once.** A person's fix and a confident correction of the agent's each
  add or strengthen an entry as they are made. The page lists what it taught itself, newest first, with why, for a
  person to review and prune; people also edit it directly. Every change is a version, as for any page.
- Every correction and every transcription reads it. A fix that names a person or a term the company doc does not know
  is also suggested to the company doc's section, for a person to accept on its page (`crate::company_doc::propose`,
  `docs/knowledge.md`, Pages, Suggestions).

## The meeting's page

A meeting is a page of the knowledge base (In the knowledge base, below), and opens its review.

- **Two columns.** The transcript on the left, three quarters of the width. On the right, one lane: an options box that
  floats at the top, above everything in the lane, and the comments below it, which pass under the box as they scroll
  by. The options box switches the transcript between Final and each capture's own (one click each, a capture read
  exactly as it came), turns each kind of mark on or off, shows the selection's span and length, and holds the
  meeting's actions (privacy, sharing, captures and their offsets, Done reviewing).
- **Along the top**, the meeting's tasks (Tasks, below) and its player.
- On a phone the right lane becomes a drawer.

### Marks

| Mark          | What it is                                                       | How it shows                              |
|---------------|------------------------------------------------------------------|-------------------------------------------|
| Confidence    | A flag of the agent's, or a range that changed                   | The words' background, on one continuous scale from red (0) through yellow (50) to green (100) |
| Edit          | Words a person changed, which no capture said                    | A solid purple outline                    |
| Excerpt       | A range taken into projects (Excerpts, below)                    | A blue bar in the left gutter, one lane per overlapping excerpt, the projects on hover; selected, its words are outlined in blue and its projects open in the right lane |
| Black-out     | A range that does not matter (Black-outs, below)                 | Black boxed, its text dimmed              |
| Comment       | A thread on a range                                              | A white outline, the thread in the right lane |

A mark the agent made says so, and one a person made names them.

### Clicking, selecting, and acting

- **A click picks a moment.** Clicking a word seeks the player to it. When marks lie under the word, the shortest is
  selected and every one there shows as a pill beside it, which Tab moves between; the selected mark opens in the right
  lane. Clicking where no mark is selects nothing.
- **A selection is a range of whole words.** Drag across words, or click a word and shift+click another, which reaches
  across pages of scrolling. A small bar floats beside it: Edit, Comment, Assign to a project, Black out, Pin a frame,
  and Play from here, with a key for each (E, C, P, B, F, and Space) while focus is not in a text input, and Escape
  clearing it.
- **Editing.** Edit opens the range's text in place, with the agent's alternatives above it as one-click choices when
  it has any. Choosing one, or typing and pressing ctrl+Enter, applies it.
- **Everything is an action.** Each change applies at once and confirms with a toast carrying Undo, as attributing a
  record does today (`docs/frontend.md`, Saving); the meeting's page has no Save. Ctrl+Z undoes the page's last change.
- **What an edit does.** It replaces its range's words with new ones, marked as the person's. Excerpts, comments, and
  pins over the old words cover the new ones. The Transcription doc learns from it, every excerpt over it shows and
  indexes the new text, and nothing is cut again until Done reviewing (The flow, below).

### The player

- The meeting's media plays along the top. Once the page scrolls past it, it floats in a corner, dragged anywhere and
  remembered per viewer, and keeps playing.
- It plays one capture's media at a time, the best video by default; a switcher changes capture at the same moment on
  the meeting's clock. A capture with audio alone plays over black with its waveform.
- Clicking a word seeks it. Left and right arrows jump ten seconds back and forward, and Space plays and pauses, while
  focus is not in a text input. The word being spoken is underlined as it plays.
- The media route answers range requests, so a two hour recording seeks without downloading it whole.

## Excerpts

An excerpt is a range of words and the projects it is about. It replaces today's topics, which cut the whole meeting
into stretches: the point is to take only what matters.

- Segmenting makes them tight: each one the words that matter to a project, not the stretch of meeting around them.
  Hellos, small talk, and asides belong to no excerpt.
- Excerpts may overlap and nest: one sentence can be its own excerpt on project B inside a topic on project A.
- Each excerpt is a timeline record on its projects, with its clip, its words, its pins, and its attachments, its
  history, and undo, as a segment's record is today (`docs/meetings.md`, Segment records). A project's page shows each
  excerpt it is on, opening the meeting at it.
- People select a range and assign it, change an excerpt's projects, or remove it. Elysia never overrides a person's
  assignment, as today.
- Only excerpts reach Project Managers and workers. An agent never reads a whole meeting unless it asks for one.
- The topics meetings were segmented into before become excerpts over their words, keeping their records,
  projects, and history.

## Black-outs

A black-out is a range that truly does not matter: "is my mic on?", "how do I share my screen?", "how was your
weekend?". Segmenting applies them on its own, and people add, change, and remove them as they review.

- A black-out leaves its words out of search and out of everything an agent reads. Everything not blacked out stays
  in search, excerpt or not: what seemed irrelevant when it was said may be what a search needs later.
- A black-out still shows on the page, and still plays.

## Tasks

- Segmenting marks each task it hears as a range, with who owes it and the project it is for.
- **A project's tasks go to its Project Manager**, with their ranges and the excerpts around them, not the meeting.
  It makes the action items, each linked to its range, or says why it made none. They arrive as its changeset, under
  its project's level of autonomy (`docs/autonomy.md`, The dial), so at the default they wait for a person.
- **The person's own tasks with no project**, outside coding, become action items of theirs at once.
- **A coworker's tasks** go to them when they are on Elysium, or to the person as waiting on them when not.
- **Tasks no project fits** are Other.
- The bar along the top counts them: each project with how many tasks it has from the meeting, the person's own, each
  coworker's, and Other, each opening its items. Each item links back to its range.

## Comments

Threads anchored to a range, read by everyone who can read the meeting: replies, resolving and reopening, and
@mentions, each raising an inbox item for who it concerns: whoever a comment mentions, and everyone else taking part in
the thread for a reply, a resolve, or a reopen. @Elysia in a thread is answered there, and she can do what it asks,
such as "this was Sharath" or "this belongs to the launch". Comments are indexed with the meeting, and an excerpt's
comments travel with it to its projects: today a meeting is indexed as its segments, and each thread with the segments
its range overlaps, so the excerpts that replace segments carry the threads over them.

## Attachments and frames

- **Attachments.** Any file a meeting was about, a deck, a PDF, a spec, is attached to it, a frozen copy kept with
  that meeting, so five meetings over a document keep the version each discussed. Its text is extracted and indexed
  with the meeting. Each attachment may be pinned to excerpts, and travels with them to their projects. The largest
  is an admin setting, 1 GB by default, in the attachment storage location page attachments use.
- **Frames.** A frame is a still from a capture's video at a moment, pinned to the meeting with a caption. People pin
  one from the player. The segmenting agent pins its own: it is offered the frames where the screen changes most (a new
  slide, a new window), may ask for any frame by its time, and judges what each shows against what was said then,
  encouraged to hand frames to subagents to look at. Near-duplicates are dropped. A frame's caption and its text are
  indexed, and a frame pinned to an excerpt travels with it to Project Managers and workers.

## The flow

The `meeting` workflow becomes:

| Step                 | What happens                                                                               |
|----------------------|--------------------------------------------------------------------------------------------|
| `fetch_meeting`      | Stores a capture as its source gave it, and its media                                      |
| `transcribe_meeting` | Whisper transcribes each capture with audio, with the hint words                           |
| `align_meeting`      | Puts every capture on the meeting's clock and lines up their words                         |
| `correct_transcript` | Settles the final transcript, flags what it is unsure of, and decides who sees the meeting |
| `segment_meeting`    | Excerpts, black-outs, tasks, and frames, then hands each project its own                   |

- **Held when unsure.** A meeting waits for its person before segmenting when its source asks (`waitForApproval`, as
  today), or when the correction left a range below a confidence the code fixes (40). An action item asks them to
  review it. Otherwise it flows on, and review comes after.
- **Done reviewing** segments the meeting again from its corrected words. Every excerpt, black-out, and task a person
  made or changed stands; what changed reaches the Project Managers it concerns, and nothing else wakes.
- A new capture merged into a meeting aligns and settles it again, keeping every person's edit.

## Uploads

- From the Meetings page (a new meeting) or a meeting's page (a capture of it): recordings as `.mp3`, `.wav`, `.mp4`,
  or `.mov`, and transcripts as WebVTT, SRT, or plain text and Markdown, where `Speaker: line` and `[00:01:23]` are read
  when they are there. A recording and a transcript can be one capture.
- Uploads are resumable, in parts, into the storage location the person keeps meetings in, since a recording can be
  gigabytes.
- An upload is a capture like any other: transcribed, aligned, corrected, and segmented. A transcript with no times
  and no media is a meeting in words alone: reviewed, excerpted, and commented, with no player.
- How it works today is `docs/meetings.md`, Uploads. A capture's upload is among the actions in the meeting's options;
  when the knowledge base's own Meetings page is built (#235), a new meeting's upload moves there.

## Teams and Slack huddles

The Teams and Slack connectors already sign in through their browsers (`docs/connectors.md`); each gains "Read meeting
recordings and transcripts", on by default, and a storage location, and provides meetings.

- **Teams** reads each recorded meeting the person can reach, its recording and its transcript (WebVTT, with speakers),
  through the connector's browser.
- **Slack huddles** have no recording. Slack keeps a huddle's transcript in its huddle notes when the workspace's plan
  turns them on; Elysium reads the transcript part alone, never Slack's summary, as no source's own AI writing is read.
  A huddle is a meeting in words alone.

## In the knowledge base

Every doc appears in the knowledge base, meetings included. Settings keeps only setting sources up.

- **Meetings**, a fixed page, lists every meeting the reader can see, newest first, with search, filters by source,
  state, and project, and how many wait for review. It is each meeting's home, unattributed ones included, and where
  uploads start.
- **Each project's page is its brief**, under the fixed Project briefs page, made and renamed with its project
  (`docs/autonomy.md`, The project brief). Under each, a Meetings group lists every meeting with an excerpt on the
  project, newest first, as linked entries of the one meeting page, so a meeting about three projects shows in three
  places and is one page. Deleting the project leaves its brief an ordinary page, and takes the group with it. Until
  excerpts replace topics, a meeting is in a project's group when one of its topics is on the project.
- The meeting's page is in the knowledge base, `/knowledge/meetings/:meetingId`, and its Settings address lands there.
  Settings, Meetings keeps only its Sources.

Built: the Meetings page, the groups under each brief, and the tree's rules for their entries (`docs/knowledge.md`,
The tree, Linked entries), listing a person's own meetings until meetings are public by default.

## Roadmap

Five lanes, in order. Each issue is whole on its own; what it depends on is named in it.

| Phase | Issue | Lane                                                                                  |
|-------|-------|---------------------------------------------------------------------------------------|
| 1     | #234  | The meeting's page: layout, marks, selecting, and editing                             |
| 1     | #236  | The player                                                                            |
| 2     | #239  | Settling the final transcript, and the Transcription doc                              |
| 3     | #241  | Excerpts, black-outs, holding when unsure, and Done reviewing                         |
| 3     | #243  | Tasks through Project Managers                                                        |
| 3     | #245  | Comments                                                                              |
| 4     | #246  | Public by default, private until checked, and sharing                                 |
| 5     | #248  | Teams recordings and Slack huddle transcripts                                         |
| 5     | #250  | Attachments and frames                                                                |

Phase 1 is what lets a person add and fix their own meetings; the rest makes the transcript better, takes only what
matters into projects, and brings in more sources.
