// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `PolicyResponse` in api/src/routes/v1/meeting_attachments/mod.rs: what a meeting takes
// as a file or a frame. All but the largest file are constants in the API's code; the largest is
// an admin's setting.
export type MeetingAttachmentPolicy = {
  // Where files and frames go: the location page attachments use; null until an admin chooses one.
  destinationId: string | null
  maxBytes: number
  maxBytesCeiling: number
  partBytes: number
  maxPerMeeting: number
  frameMaxBytes: number
  captionMaxCharacters: number
  textMaxCharacters: number
}

type PolicyResponse = {
  policy: MeetingAttachmentPolicy
}

export const MEETING_ATTACHMENT_POLICY_KEY = 'v1/meeting-attachments'

export function getMeetingAttachmentPolicy() {
  return apiClient
    .get(MEETING_ATTACHMENT_POLICY_KEY)
    .json<PolicyResponse>()
}

// An admin chooses the largest file a meeting takes, from one byte to `maxBytesCeiling`.
export function setMeetingAttachmentLimit(maxBytes: number) {
  return apiClient
    .put(`${MEETING_ATTACHMENT_POLICY_KEY}/limit`, { json: { maxBytes }})
    .json<PolicyResponse>()
}
