// Copyright © 2026 Jalapeno Labs

import type { MeetingUpload, MeetingUploadFileKind } from '../../api/routes/meetingUploadRoutes'
import type { ChosenFiles } from './meetingUpload'

// Utility
import { HTTPError, NetworkError } from 'ky'

// Misc
import { getApiErrorCode } from '../../api/errors'
import { getMeetingUpload, uploadMeetingPart } from '../../api/routes/meetingUploadRoutes'
import {
  MEETING_UPLOAD_FIRST_RETRY_DELAY_MS,
  MEETING_UPLOAD_MAX_RETRY_DELAY_MS,
  MEETING_UPLOAD_PART_ATTEMPTS,
} from '../../constants'
import { nextPart, partCount, retryDelay } from './meetingUpload'

// Answers the API gives while it is busy or briefly away, which a part waits out and tries again:
// too many requests, a request it had no time for, and the upstream it stages in not answering.
const RETRIED_STATUSES = new Set([ 408, 429, 502, 503, 504 ])

// The transcript goes first: it is small, and the API reads it as its last part arrives, so one it
// cannot read is refused before a recording of gigabytes is sent after it.
const SEND_ORDER = [ 'transcript', 'recording' ] as const satisfies readonly MeetingUploadFileKind[]

type Options = {
  upload: MeetingUpload
  files: ChosenFiles
  signal: AbortSignal
  // Each time a part lands, with the upload as the API now counts it.
  onProgress: (upload: MeetingUpload) => void
  // Waits `delayMs`, ending early when `signal` aborts; tests pass one that does not wait.
  wait?: (delayMs: number, signal: AbortSignal) => Promise<void>
}

// Sends every part of `files` that `upload` has not received, in order, one at a time, the
// transcript first, and answers the upload once all of it has arrived. A part that fails on the
// way (a dropped connection, the API busy) waits and is sent again, as many as
// `MEETING_UPLOAD_PART_ATTEMPTS` times; a part the API says is ahead of where it is reads the
// upload again and carries on from there. Any other refusal ends it, for the page to say why.
// Nothing is held but the part on its way: each is a slice of the file the browser reads as it
// sends.
export async function sendMeetingFiles(options: Options) {
  const {
    files,
    signal,
    wait = waitFor,
  } = options
  let upload = options.upload
  for (const kind of SEND_ORDER) {
    const file = files[kind]
    if (!file) {
      continue
    }
    const count = partCount(file.size, upload.partBytes)
    let failures = 0
    let index = partToSend(upload, kind)
    while (index < count) {
      signal.throwIfAborted()
      const start = index * upload.partBytes
      const part = file.slice(start, Math.min(start + upload.partBytes, file.size))
      try {
        const response = await uploadMeetingPart(upload.id, kind, index, part, signal)
        upload = response.upload
        failures = 0
        options.onProgress(upload)
      }
      catch (error) {
        signal.throwIfAborted()
        failures += 1
        if (failures >= MEETING_UPLOAD_PART_ATTEMPTS) {
          throw error
        }
        if (getApiErrorCode(error) === 'meeting_upload_part_ahead') {
          // Another tab, or an earlier answer lost on the way, moved it on: carry on from there.
          upload = (await getMeetingUpload(upload.id)).upload
          options.onProgress(upload)
        }
        else if (isPassing(error)) {
          console.debug('sendMeetingFiles waits to send a part again', { kind, index, failures, error })
          await wait(
            retryDelay(
              failures,
              retryAfterOf(error),
              MEETING_UPLOAD_FIRST_RETRY_DELAY_MS,
              MEETING_UPLOAD_MAX_RETRY_DELAY_MS,
            ),
            signal,
          )
        }
        else {
          throw error
        }
      }
      index = partToSend(upload, kind)
    }
  }
  return upload
}

// The part of `upload`'s file of `kind` it needs next.
function partToSend(upload: MeetingUpload, kind: MeetingUploadFileKind) {
  const stored = upload.files.find((file) => file.kind === kind)
  if (!stored) {
    throw new Error(`the upload carries no ${kind}`)
  }
  return nextPart(stored, upload.partBytes)
}

// Whether `error` is one that passes: the connection dropped before an answer, or an answer that
// says to try again shortly.
function isPassing(error: unknown) {
  if (error instanceof HTTPError) {
    return RETRIED_STATUSES.has(error.response.status)
  }
  return error instanceof NetworkError
}

// How many seconds a `429` asks to wait, when it says.
function retryAfterOf(error: unknown) {
  if (!(error instanceof HTTPError)) {
    return null
  }
  const seconds = Number(error.response.headers.get('Retry-After'))
  return Number.isFinite(seconds) && seconds > 0
    ? seconds
    : null
}

function waitFor(delayMs: number, signal: AbortSignal) {
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, delayMs)
    signal.addEventListener('abort', () => {
      clearTimeout(timer)
      resolve()
    }, { once: true })
  })
}
