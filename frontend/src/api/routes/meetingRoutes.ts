// Copyright © 2026 Jalapeno Labs

import type { ConnectorKind } from './connectorRoutes'
import type { LearningAgent } from './learningRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `Processing` in api/src/meetings/workflow.rs: how far a meeting's processing got, or
// that its run failed or went through without it, a person having skipped a step it needed.
export const MEETING_STATES = [ 'pending', 'recorded', 'segmented', 'done', 'skipped', 'failed' ] as const
export type MeetingState = typeof MEETING_STATES[number]

// What a meeting says of its corrected transcript.
export type MeetingCorrection = {
  changedLines: number
  // When its person approved it, and who did; null until then.
  approvedAt: string | null
  approvedBy: string | null
  // The inbox item raised to review it, while that item exists.
  reviewItemId: string | null
  createdAt: string
}

// A meeting read from a meeting source. Its transcript is loaded on its own page
// (`getMeetingWords`); its segments reach the timeline. Only its owner receives it.
export type Meeting = {
  id: string
  // The connector that read it, or null once that connector is removed.
  connectorId: string | null
  // Where it came from: a connector kind, which outlives its connector, or a source that is none,
  // such as an upload; `meetingSourceNameKey` names either.
  sourceKind: string
  title: string
  startedAt: string
  durationMs: number | null
  organizer: string
  participants: string[]
  // Its page at the source.
  url: string | null
  state: MeetingState
  // Why its run failed, while it has.
  lastError: string | null
  hasRecording: boolean
  // How many segments Elysia found, or null before she has read it.
  segmentCount: number | null
  // Whether its person asked to approve its corrected transcript before it is acted on.
  holdForApproval: boolean
  // Its corrected transcript, or null before it is corrected.
  correction: MeetingCorrection | null
  // Its newest run of the meeting workflow, or null before it has one.
  runId: string | null
  // Whether that run is held, waiting for its person to approve the corrected transcript.
  awaitingApproval: boolean
  // The meeting its captures were merged into, while they are: it is listed nowhere then, and
  // they are split back out from there.
  mergedInto: string | null
  createdAt: string
  updatedAt: string
}

// Mirrors `TranscriptLine` in api/src/meetings/source.rs: what one speaker said, and when.
export type TranscriptLine = {
  startMs: number
  endMs: number
  // Empty when the transcript names no speaker, as Whisper's never does.
  speaker: string
  text: string
}

// Mirrors `BasedOn` in api/src/models/meeting_correction.rs: which transcript a capture's own
// is, its source's lines or, for a source that sent none, Whisper's segments.
export type TranscriptBase = 'source' | 'whisper'

// Mirrors `WordResponse` in api/src/routes/v1/meetings/mod.rs: one word of a meeting's final
// transcript, on the meeting's clock.
export type MeetingWord = {
  id: string
  // Orders the transcript; a change's new words slot in by it.
  position: number
  text: string
  speaker: string
  startMs: number
  endMs: number
  // `source`, `whisper`, `correction`, or `edit`.
  origin: string
  // The capture it was heard in; null for an edit's words.
  captureId: string | null
}

// Mirrors `MarkKind` in api/src/models/meeting_mark.rs. A kind a later API adds arrives as its
// stored name and is left undrawn.
export const MEETING_MARK_KINDS = [ 'confidence', 'edit', 'excerpt', 'black_out', 'comment', 'pin', 'task' ] as const
export type MeetingMarkKind = typeof MEETING_MARK_KINDS[number]

// Mirrors `MarkResponse`: one mark over a range of the final transcript, from its first word to
// its last. Its data is shaped by its kind (docs/meetings.md, The final transcript).
export type MeetingMark = {
  id: string
  kind: string
  startWordId: string
  endWordId: string
  data: Record<string, unknown>
  createdBy: string
  // The agent that acted for whoever made it, if one did.
  agent: LearningAgent | null
  createdAt: string
}

// Mirrors `OffsetSource` in api/src/models/meeting_capture.rs: how a capture's offset onto its
// meeting's clock was found. One set by `hand` is never moved by aligning.
export const CAPTURE_OFFSET_SOURCES = [ 'listed', 'audio', 'words', 'hand' ] as const
export type CaptureOffsetSource = typeof CAPTURE_OFFSET_SOURCES[number]

// One capture of a meeting, without what it holds.
export type MeetingCapture = {
  id: string
  connectorId: string | null
  sourceKind: string
  url: string | null
  // Where its clock sits on the meeting's: its start, in milliseconds into the meeting.
  offsetMs: number
  // How that was found; a source a later API adds arrives as its stored name.
  offsetSource: string
  hasRecording: boolean
  // The meeting a merge brought it from, while that merge stands; null for the meeting's own.
  // Splitting it out makes that meeting its own again, with every capture the merge brought.
  mergedFrom: {
    meetingId: string
    title: string
  } | null
}

// A meeting's final transcript as it reads: its standing words in order, the marks over them,
// and its captures. No words until it is built, which its correction does.
export type MeetingWords = {
  words: MeetingWord[]
  marks: MeetingMark[]
  captures: MeetingCapture[]
}

// What one change did to the transcript, to apply without reading it again: the words it took
// out, those it stood in their place, and every mark as it now stands.
export type MeetingTranscriptChange = {
  removedWordIds: string[]
  words: MeetingWord[]
  marks: MeetingMark[]
}

// Mirrors `MeetingEventResponse`: one entry of a meeting's history.
export type MeetingHistoryEntry = {
  id: string
  // `built`, `edited`, `marked`, `unmarked`, `hold_changed`, `correction_approved`,
  // `offset_changed`, `merged`, or `split`.
  kind: string
  createdBy: string
  agent: LearningAgent | null
  // Shaped by its kind (docs/meetings.md, History); read defensively, since entries outlive the
  // version that wrote them.
  data: Record<string, unknown>
  // The entry this one undid, when it is an undo.
  undid: string | null
  createdAt: string
}

// A capture's own transcript as it came, on the meeting's clock; null while nothing was heard.
export type CaptureTranscript = {
  transcript: {
    basedOn: TranscriptBase
    lines: TranscriptLine[]
  } | null
}

// What an edit or an undo answers: its history entry, and what it did to the transcript.
export type MeetingChangeResponse = {
  entry: MeetingHistoryEntry
  change: MeetingTranscriptChange
}

type ListMeetingsResponse = {
  meetings: Meeting[]
}

export function listMeetings() {
  return apiClient
    .get('v1/meetings')
    .json<ListMeetingsResponse>()
}

type MeetingResponse = {
  meeting: Meeting
}

export function getMeeting(meetingId: string) {
  return apiClient
    .get(`v1/meetings/${meetingId}`)
    .json<MeetingResponse>()
}

// The meeting whose corrected transcript an inbox item asks its person to review, if it is the
// signed-in person's.
export function listMeetingsReviewedBy(itemId: string) {
  return apiClient
    .get('v1/meetings', { searchParams: { reviewItemId: itemId }})
    .json<ListMeetingsResponse>()
}

// Resumes a failed meeting from the step it failed at.
export function retryMeeting(meetingId: string) {
  return apiClient
    .post(`v1/meetings/${meetingId}/retry`)
    .json<MeetingResponse>()
}

// SWR keys of what a meeting's page reads, which the event stream revalidates when the meeting
// changes.
export function meetingWordsKey(meetingId: string) {
  return `v1/meetings/${meetingId}/words`
}

export function meetingHistoryKey(meetingId: string) {
  return `v1/meetings/${meetingId}/history`
}

export function getMeetingWords(meetingId: string) {
  return apiClient
    .get(meetingWordsKey(meetingId))
    .json<MeetingWords>()
}

export function getCaptureTranscript(meetingId: string, captureId: string) {
  return apiClient
    .get(`v1/meetings/${meetingId}/captures/${captureId}/transcript`)
    .json<CaptureTranscript>()
}

type EditMeetingWordsRequest = {
  startWordId: string
  endWordId: string
  // Split at whitespace into the new words; at least one.
  text: string
}

// Replaces a range of the final transcript. `409 meeting_words_changed` when a word of it was
// replaced meanwhile.
export function editMeetingWords(meetingId: string, body: EditMeetingWordsRequest) {
  return apiClient
    .post(`v1/meetings/${meetingId}/edits`, { json: body })
    .json<MeetingChangeResponse>()
}

type MeetingHistoryResponse = {
  entries: MeetingHistoryEntry[]
}

// Newest first.
export function getMeetingHistory(meetingId: string) {
  return apiClient
    .get(meetingHistoryKey(meetingId))
    .json<MeetingHistoryResponse>()
}

// `409 meeting_changed_since` when what it changed moved on since.
export function undoMeetingChange(meetingId: string, entryId: string) {
  return apiClient
    .post(`v1/meetings/${meetingId}/history/${entryId}/undo`)
    .json<MeetingChangeResponse>()
}

// Approves the corrected transcript, and lets a run held for it go on.
export function approveMeetingCorrection(meetingId: string) {
  return apiClient
    .post(`v1/meetings/${meetingId}/correction/approve`)
    .json<MeetingResponse>()
}

// Holds the meeting for its person's approval of its corrected transcript, or stops holding
// it. `409 meeting_corrected` once it is corrected.
export function setMeetingHold(meetingId: string, hold: boolean) {
  const path = `v1/meetings/${meetingId}/hold`
  const request = hold
    ? apiClient.post(path)
    : apiClient.delete(path)
  return request.json<MeetingResponse>()
}

// Two meetings that look like one, as an inbox item asks its person about them: the meeting
// asked about, whose captures a merge moves, and the one it looks like. Mirrors
// `DuplicateResponse` in api/src/routes/v1/meetings/get_duplicate.rs.
export type MeetingDuplicate = {
  id: string
  meetingId: string
  meetingTitle: string
  // Where each meeting's first capture came from, which tells two meetings of one title apart;
  // null for the meeting asked about once its captures were merged into the other.
  meetingSourceKind: ConnectorKind | null
  sameAsId: string
  sameAsTitle: string
  sameAsSourceKind: ConnectorKind | null
  // Null until its person decides.
  decision: 'merged' | 'not_the_same' | null
  decidedAt: string | null
}

type MeetingDuplicateResponse = {
  duplicate: MeetingDuplicate | null
}

// The two meetings an inbox item asks the signed-in person about, or null when it asks about
// none of theirs.
export function getMeetingDuplicateAskedBy(itemId: string) {
  return apiClient
    .get('v1/meetings/duplicates', { searchParams: { itemId }})
    .json<MeetingDuplicateResponse>()
}

type DecidedMeetingDuplicateResponse = {
  duplicate: MeetingDuplicate
}

// Merges the meeting asked about into the one it looks like. `409 duplicate_decided` once the
// pair is decided.
export function mergeMeetingDuplicate(duplicateId: string) {
  return apiClient
    .post(`v1/meetings/duplicates/${duplicateId}/merge`)
    .json<DecidedMeetingDuplicateResponse>()
}

// Records that the two are different meetings, so they are never asked about again.
export function keepMeetingDuplicateApart(duplicateId: string) {
  return apiClient
    .post(`v1/meetings/duplicates/${duplicateId}/not-the-same`)
    .json<DecidedMeetingDuplicateResponse>()
}

// Mirrors `OFFSET_MAX_MS` in api/src/meetings/clock.rs: the furthest a capture may sit from its
// meeting's clock, either way, so a slip of the keyboard is caught before it is sent.
export const CAPTURE_OFFSET_MAX_MS = 12 * 60 * 60 * 1000

// What moving a capture on the clock answers: its history entry, which Undo undoes, and where the
// capture now sits. The transcript is lined up again, so the page reads it again rather than
// applying a change.
type SetCaptureOffsetResponse = {
  entry: MeetingHistoryEntry
  capture: {
    id: string
    offsetMs: number
    offsetSource: string
  }
}

// Sets where capture `captureId` starts on its meeting's clock, by hand, which aligning never
// moves after. `400 offset_out_of_range` past `CAPTURE_OFFSET_MAX_MS` either way.
export function setCaptureOffset(meetingId: string, captureId: string, offsetMs: number) {
  return apiClient
    .put(`v1/meetings/${meetingId}/captures/${captureId}/offset`, { json: { offsetMs }})
    .json<SetCaptureOffsetResponse>()
}

// What splitting answers: its history entry, which Undo undoes by merging again, and the meeting
// as it now stands.
type SplitCaptureResponse = {
  entry: MeetingHistoryEntry
  meeting: Meeting
}

// Splits capture `captureId` back out of the meeting a merge brought it into, with every capture
// that merge brought. `409 capture_not_merged` for one no merge brought, `409 meeting_changed`
// once a capture it brought was moved by hand since.
export function splitCapture(meetingId: string, captureId: string) {
  return apiClient
    .post(`v1/meetings/${meetingId}/captures/${captureId}/split`)
    .json<SplitCaptureResponse>()
}
