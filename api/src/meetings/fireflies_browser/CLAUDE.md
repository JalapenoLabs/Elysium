# Fireflies through a browser

Read this before changing anything in `api/src/meetings/fireflies_browser/`, and add to it whenever Fireflies breaks
the reader. How the source works is in `docs/meetings.md`, Fireflies through a browser; this file is what the next
person to fix it needs.

## How it reads Fireflies, and why

- Fireflies' public API (`api.fireflies.ai/graphql`, the key kind in `../fireflies.rs`) refuses transcripts and
  recording addresses below its paid plans. Its web app reads everything, on any plan, from its own GraphQL endpoint,
  `POST /api/v4/hive` on `app.fireflies.ai`. So this source asks that endpoint, from the connector's own signed-in
  browser, over DevTools.
- `hive.js` is a relay and nothing more: it takes `{ calls: [{ query, variables }] }`, refuses anything that is not a
  query, asks each one in order, and answers `{ signedIn, results: [{ status, data, errors }] }`. The queries live in
  `mod.rs` as constants, so the AI-field test in `../fireflies.rs` (`no_query_asks_for_anything_fireflies_ai_wrote`)
  reads them.
- The web app keeps its session token in `localStorage.AUTHORIZATION` and sends it as the `authorization` header (no
  `Bearer`), beside its cookies. The script sends the same, and never returns it. A page with no token is signed out.
- `mod.rs` deserializes only the fields it uses. Unknown fields are ignored, so a field Fireflies adds never breaks a
  read.

## Where the queries come from

None of this is documented. Every query name and shape was read from the web app's own JavaScript, which anyone can
fetch without signing in: `curl https://app.fireflies.ai/notebook/mine`, collect the
`dashboard-ff-cdn.fireflies.ai/<build>/_next/static/chunks/*.js` it names (and those of `/view/<any id>`), and grep
them for `query <name>`. The chunks are CORS-blocked from the page, so fetch them with `curl`. Chunk names change with
every build; operation names have not.

| What                | The web app's operation                | Read as                                                     |
|---------------------|----------------------------------------|-------------------------------------------------------------|
| The notebook        | `getFilteredMeetings(from, size, channelId, filters, sort)` | `ElysiumMeetings`: `id title date durationMins creator_email allEmails processMeetingStatus` |
| A meeting's words   | `meetingNote(_id)` (`fetchMeetingInfo`, the meeting page) | `ElysiumTranscript`: `processMeetingStatus speakerMeta sentenceMeta labelMeta captions { sentence speaker_id time endTime }` |
| A meeting's media   | `meetingNote(_id)`                     | `ElysiumRecording`: `video_url audio_url`                   |

- `channelId` names the notebook's tab: the web app maps "My Meetings" to `USER_PART_OF`, "Hosted by me" to `mine`,
  "All Meetings" to `all`, and "Shared with me" to `shared`. The source reads `USER_PART_OF`.
- The web app's own validation (zod, in the chunks) allows `from` 0 to 30000, `size` 1 to 150, `filters.date` as
  `{ from, to? }` strings any `new Date()` parses, and `sort` as `[{ field, order: ASC | DESC }]`. The notebook sorts on
  `date`.
- A meeting's page is `/view/<id>`, where the id is the listing's `id`, which `meetingNote(_id:)` takes.

Never ask for what Fireflies' AI wrote. Queries that look useful carry it: `GetFeedMeetings` (`brief { gist overview }`,
`summary`), `globalSearch` (`shortSummary`), `fetchMeetingInfo` as the web app sends it (`summary`, `summaryStatus`).
Write a selection of your own instead, from fields the app's queries show exist.

## Shapes

| Field                         | Type                                                                        |
|-------------------------------|-----------------------------------------------------------------------------|
| `captions[].time`, `endTime`  | seconds from the recording's start, as numbers                              |
| `captions[].speaker_id`       | integer                                                                     |
| `speakerMeta`                 | an object keyed one past `speaker_id`: `{ "1": "Ada" }`; a missing key shows as `Speaker <id + 1>` |
| `sentenceMeta`                | an object keyed by caption index: the edited sentence; `""` deleted it      |
| `labelMeta`                   | an object keyed by speaker id: the caption indexes reassigned to them       |
| `processMeetingStatus`        | `not_started`, `started`, `completed`, or `failed`                          |
| listing `date`                | ISO 8601 text in UTC, such as `2026-10-12T19:00:00Z`; milliseconds are read too |
| listing `durationMins`        | a number with a fraction, such as `26.63`                                   |
| listing `allEmails`           | one string of addresses, read as comma separated; a list is read too        |
| `video_url`, `audio_url`      | on the free plan, `video_url` is `null` and `audio_url` a signed address on `cdn.fireflies.ai` |

Errors come back over HTTP 200 with `extensions.code`: `UNAUTHENTICATED` when the session ended, `NOT_FOUND_ERROR` for
a meeting this account cannot open.

## Quirks

- Fireflies lists a meeting as soon as it is recorded, before its words exist. A listing stops at the first meeting
  still `not_started` or `started` (`settled`), so the cursor waits before it, unless it has been so for a day.
- The web app shows the transcript after a cleanup that drops immediately repeated words ("sure, sure" as "sure").
  The source keeps Fireflies' raw words, which are what was heard; don't port the cleanup.
- `durationMins` overstates the recording by a few percent; the recording's own length is the true one.
- The app refreshes its own token (`/api/auth/refresh-auth-token`) when it is within a week of its two-week expiry,
  and only as it loads the signed-in user, so a tab that is never reloaded signs out after two weeks. The source never
  refreshes it; the daily browser refresh (`crate::connectors::refresh`, which reaches every browser kind) reloads the
  Fireflies tab, and the app refreshes it then.

## What a signed-in session showed

All three queries were asked, as written in `mod.rs`, of a free-plan account's own signed-in tab on 2026-11-10:

- The listing answered 28 meetings for 30 days, every one `completed`, in the shapes above.
- Full transcripts come back on the free plan: across ten meetings, the last caption ended within a minute or two of
  `durationMins` (888 captions to 57.3 minutes of a 58.8 minute meeting).
- The recording query answers no error on the free plan (no `paid_required`), with audio alone.

Not yet verified: that the audio address downloads whole from the engine, outside the browser. A ranged fetch from the
page fails on CORS, which says nothing about a download from the server.
