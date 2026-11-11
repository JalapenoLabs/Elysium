// Copyright © 2026 Jalapeno Labs

import type { Meeting } from './meetingRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `FileKind` in api/src/meeting_uploads/mod.rs: what one file of an upload is.
export const MEETING_UPLOAD_FILE_KINDS = [ 'recording', 'transcript' ] as const
export type MeetingUploadFileKind = typeof MEETING_UPLOAD_FILE_KINDS[number]

// What a meeting takes, and where its recording goes unless the person chooses another place.
// Constants in the API's code, read from it so nothing keeps a second copy.
export type MeetingUploadPolicy = {
  // How many bytes each part of a file carries but the last.
  partBytes: number
  recording: MeetingUploadFilePolicy
  transcript: MeetingUploadFilePolicy
  // Where the owner already keeps meetings, or null when they keep them nowhere yet.
  defaultStorageLocationId: string | null
}

export type MeetingUploadFilePolicy = {
  maxBytes: number
  // Lowercase, without their dots.
  extensions: string[]
}

export type MeetingUploadFile = {
  kind: MeetingUploadFileKind
  fileName: string
  sizeBytes: number
  // When the file last changed, as the browser read it.
  modifiedAt: string | null
  // How much of it arrived, in whole parts from its start.
  receivedBytes: number
}

// An upload while its parts arrive, and once it became a capture.
export type MeetingUpload = {
  id: string
  // The meeting it adds a capture to; null for a new meeting.
  meetingId: string | null
  title: string | null
  startedAt: string | null
  storageLocationId: string | null
  partBytes: number
  files: MeetingUploadFile[]
  captureId: string | null
  finishedAt: string | null
  createdAt: string
  updatedAt: string
}

type PolicyResponse = {
  policy: MeetingUploadPolicy
}

// What a meeting takes; with `meetingId`, where a capture of that meeting's recording goes.
export function getMeetingUploadPolicy(meetingId: string | null) {
  return apiClient
    .get('v1/meeting-uploads/policy', {
      searchParams: meetingId
        ? { meetingId }
        : {},
    })
    .json<PolicyResponse>()
}

type ListMeetingUploadsResponse = {
  uploads: MeetingUpload[]
}

// The signed-in person's unfinished uploads, the most recently moved first.
export function listMeetingUploads() {
  return apiClient
    .get('v1/meeting-uploads')
    .json<ListMeetingUploadsResponse>()
}

export type NewMeetingUploadFile = {
  name: string
  sizeBytes: number
  modifiedAt: string | null
}

// A new meeting names its title and when it started; a capture names its meeting instead.
export type CreateMeetingUploadRequest = {
  meetingId?: string
  title?: string
  startedAt?: string
  storageLocationId?: string
  recording?: NewMeetingUploadFile
  transcript?: NewMeetingUploadFile
}

type MeetingUploadResponse = {
  upload: MeetingUpload
}

export function createMeetingUpload(body: CreateMeetingUploadRequest) {
  return apiClient
    .post('v1/meeting-uploads', { json: body })
    .json<MeetingUploadResponse>()
}

export function getMeetingUpload(uploadId: string) {
  return apiClient
    .get(`v1/meeting-uploads/${uploadId}`)
    .json<MeetingUploadResponse>()
}

// Sends part `index` of an upload's file. A part already received answers the upload as it is;
// one past the next answers `409 meeting_upload_part_ahead`.
export function uploadMeetingPart(
  uploadId: string,
  kind: MeetingUploadFileKind,
  index: number,
  part: Blob,
  signal: AbortSignal,
) {
  return apiClient
    .put(`v1/meeting-uploads/${uploadId}/${kind}/parts/${index}`, {
      body: part,
      signal,
      // The upload shows its own progress and retries a part itself, so a part takes as long as
      // it takes, with neither the usual timeout and retries nor the notice that a request runs
      // long.
      timeout: false,
      retry: 0,
      fetch: (input, init) => globalThis.fetch(input, init),
    })
    .json<MeetingUploadResponse>()
}

// Makes the meeting, or the capture, once every part arrived, and starts its run.
export function finishMeetingUpload(uploadId: string) {
  return apiClient
    .post(`v1/meeting-uploads/${uploadId}/finish`)
    .json<{ meeting: Meeting }>()
}

// Stops an unfinished upload and removes what of it arrived.
export function cancelMeetingUpload(uploadId: string) {
  return apiClient.delete(`v1/meeting-uploads/${uploadId}`)
}
