// Copyright © 2026 Jalapeno Labs

import type { ConnectorKind } from './connectorRoutes'

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
// (`getMeetingTranscript`); its segments reach the timeline. Only its owner receives it.
export type Meeting = {
  id: string
  // The connector that read it, or null once that connector is removed.
  connectorId: string | null
  // The kind of connector it came from, which outlives the connector.
  sourceKind: ConnectorKind
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

// Mirrors `BasedOn` in api/src/models/meeting_correction.rs: which transcript a corrected copy
// follows line by line.
export type TranscriptBase = 'source' | 'whisper'

export type MeetingTranscript = {
  // The transcript as heard, which the corrected copy follows; null while nothing was heard.
  original: {
    basedOn: TranscriptBase
    lines: TranscriptLine[]
  } | null
  // The corrected copy, line for line; null until the meeting is corrected.
  corrected: {
    lines: TranscriptLine[]
    changedLines: number
    approvedAt: string | null
    approvedBy: string | null
  } | null
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

export function getMeetingTranscript(meetingId: string) {
  return apiClient
    .get(`v1/meetings/${meetingId}/transcript`)
    .json<MeetingTranscript>()
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
