// Copyright © 2026 Jalapeno Labs

import type {
  MeetingUpload,
  MeetingUploadFile,
  MeetingUploadFileKind,
  MeetingUploadPolicy,
} from '../../api/routes/meetingUploadRoutes'

// Misc
import { MEETING_UPLOAD_FILE_KINDS } from '../../api/routes/meetingUploadRoutes'

// The files a person chose for one upload: a recording, a transcript, or one of each.
export type ChosenFiles = Record<MeetingUploadFileKind, File | null>

// Why a meeting would refuse a file, before a byte of it is sent: not a type it takes, empty, or
// over its size.
export type FileRefusal = 'type' | 'empty' | 'tooLarge'

// A file's extension, lowercase, without its dot; empty when it has none.
export function extensionOf(name: string) {
  const dot = name.lastIndexOf('.')
  if (dot < 0) {
    return ''
  }
  return name.slice(dot + 1).toLowerCase()
}

// Why a meeting refuses `file` as its `kind`, by the API's own limits, or null when it takes it.
export function describeFileRefusal(
  file: File,
  kind: MeetingUploadFileKind,
  policy: MeetingUploadPolicy,
): FileRefusal | null {
  const taken = policy[kind]
  if (!taken.extensions.includes(extensionOf(file.name))) {
    return 'type'
  }
  if (!file.size) {
    return 'empty'
  }
  if (file.size > taken.maxBytes) {
    return 'tooLarge'
  }
  return null
}

// A new meeting's title from the name of a file of it: its stem, with the underscores and runs of
// spaces a recorder writes folded into single spaces.
export function titleFromFileName(name: string) {
  const dot = name.lastIndexOf('.')
  const stem = dot > 0
    ? name.slice(0, dot)
    : name
  return stem.replaceAll('_', ' ').replace(/\s+/g, ' ').trim()
}

// A name as the API keeps it, near enough to tell the same file chosen again: its runs of spaces
// folded and its extension in lowercase.
function comparableName(name: string) {
  const folded = name.replace(/\s+/g, ' ').trim()
  const dot = folded.lastIndexOf('.')
  if (dot < 0) {
    return folded
  }
  return folded.slice(0, dot) + folded.slice(dot).toLowerCase()
}

function isSameFile(stored: MeetingUploadFile | undefined, chosen: File | null) {
  if (!stored && !chosen) {
    return true
  }
  if (!stored || !chosen) {
    return false
  }
  const storedModifiedAt = stored.modifiedAt
    ? Date.parse(stored.modifiedAt)
    : null
  return stored.sizeBytes === chosen.size
    && storedModifiedAt === chosen.lastModified
    && comparableName(stored.fileName) === comparableName(chosen.name)
}

// What an upload is of, besides its files, as the request that starts it says it.
export type UploadDetails = Pick<MeetingUpload, 'meetingId' | 'title' | 'startedAt' | 'storageLocationId'>

function instantOf(at: string | null) {
  return at
    ? Date.parse(at)
    : null
}

function hasSameDetails(upload: MeetingUpload, details: UploadDetails) {
  return upload.meetingId === details.meetingId
    && upload.title === details.title
    && instantOf(upload.startedAt) === instantOf(details.startedAt)
    && upload.storageLocationId === details.storageLocationId
}

// The unfinished upload of these same files, as the same meeting or capture, if one was started
// before, so choosing them again carries on where it stopped rather than sending them all again.
export function findResumable(
  uploads: MeetingUpload[],
  details: UploadDetails,
  files: ChosenFiles,
): MeetingUpload | null {
  return uploads.find((upload) => hasSameDetails(upload, details)
    && MEETING_UPLOAD_FILE_KINDS.every((kind) => isSameFile(
      upload.files.find((file) => file.kind === kind),
      files[kind],
    ))) ?? null
}

// How many parts a file of `sizeBytes` arrives in.
export function partCount(sizeBytes: number, partBytes: number) {
  return Math.ceil(sizeBytes / partBytes)
}

// The part of `file` the API needs next, by its number from 0: one past its last once it is all
// there, since its last part is shorter than the rest.
export function nextPart(file: MeetingUploadFile, partBytes: number) {
  if (file.receivedBytes >= file.sizeBytes) {
    return partCount(file.sizeBytes, partBytes)
  }
  return Math.floor(file.receivedBytes / partBytes)
}

// How much of an upload arrived, across its files, as bytes over bytes.
export function uploadProgress(upload: MeetingUpload) {
  let received = 0
  let total = 0
  for (const file of upload.files) {
    received += file.receivedBytes
    total += file.sizeBytes
  }
  return { received, total } as const
}

// How long to wait before trying a part again after `attempt` failures (from 1): doubling from
// `firstDelayMs` up to `maxDelayMs`, or what the API's `Retry-After` asks, when it asks longer.
export function retryDelay(
  attempt: number,
  retryAfterSeconds: number | null,
  firstDelayMs: number,
  maxDelayMs: number,
) {
  const backoff = Math.min(firstDelayMs * 2 ** (attempt - 1), maxDelayMs)
  if (retryAfterSeconds === null) {
    return backoff
  }
  return Math.max(backoff, retryAfterSeconds * 1000)
}
