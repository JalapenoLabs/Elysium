// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `MeetingState` in api/src/models/meeting.rs: how far a meeting's processing got.
export const MEETING_STATES = [ 'pending', 'recorded', 'segmented', 'done', 'failed' ] as const
export type MeetingState = typeof MEETING_STATES[number]

// A meeting read from a meeting source. Its transcript is not sent; its segments reach the
// timeline. Only its owner receives it.
export type Meeting = {
  id: string
  connectorAccountId: string | null
  sourceKind: string
  title: string
  startedAt: string
  durationMs: number | null
  organizer: string
  participants: string[]
  // Its page at the source.
  url: string | null
  state: MeetingState
  attempts: number
  // Why the last attempt failed, or null when it succeeded.
  lastError: string | null
  hasRecording: boolean
  // How many segments Elysia found, or null before she has read it.
  segmentCount: number | null
  createdAt: string
  updatedAt: string
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

// Resumes a failed meeting from the step it failed at.
export function retryMeeting(meetingId: string) {
  return apiClient
    .post(`v1/meetings/${meetingId}/retry`)
    .json<MeetingResponse>()
}
