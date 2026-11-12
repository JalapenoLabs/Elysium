// Copyright © 2026 Jalapeno Labs

import type { MeetingUpload, UploadedFileKind } from '../../api/routes/meetingUploadRoutes'

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Utility
import { HTTPError, NetworkError } from 'ky'

// Misc
import { getMeetingUpload, uploadMeetingPart } from '../../api/routes/meetingUploadRoutes'
import { MEETING_UPLOAD_PART_ATTEMPTS } from '../../constants'
import { sendMeetingFiles } from './sendMeetingFiles'

vi.mock('../../api/routes/meetingUploadRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/meetingUploadRoutes')>(),
  uploadMeetingPart: vi.fn(),
  getMeetingUpload: vi.fn(),
}))

const PART_BYTES = 4

// A fake API that counts parts as the real one does: the next one is taken and counted, one
// received already is acknowledged, and one past the next is refused as ahead.
type FakeApi = {
  upload: MeetingUpload
  sent: { kind: UploadedFileKind, index: number, text: string }[]
  // Answers the next sends get instead of being taken, first to last.
  failures: unknown[]
}

// An upload of a recording of `recordingBytes`, `received` of them arrived, and a transcript of
// `transcriptBytes` when there is one.
function startUpload(recordingBytes: number, received = 0, transcriptBytes?: number): MeetingUpload {
  const files: MeetingUpload['files'] = [
    {
      kind: 'recording',
      fileName: 'recording.mp4',
      sizeBytes: recordingBytes,
      modifiedAt: null,
      receivedBytes: received,
    },
  ]
  if (transcriptBytes) {
    files.push({
      kind: 'transcript',
      fileName: 'transcript.txt',
      sizeBytes: transcriptBytes,
      modifiedAt: null,
      receivedBytes: 0,
    })
  }
  return {
    id: 'upload-1',
    meetingId: null,
    title: 'Board call',
    startedAt: '2026-11-09T15:00:00Z',
    storageLocationId: 'location-1',
    partBytes: PART_BYTES,
    files,
    captureId: null,
    attachmentId: null,
    finishedAt: null,
    createdAt: '2026-11-09T16:00:00Z',
    updatedAt: '2026-11-09T16:00:00Z',
  }
}

function refusal(status: number, data: unknown, headers: Record<string, string> = {}) {
  const error = new HTTPError(new Response(null, { status, headers }), new Request('http://localhost/'), {} as never)
  error.data = data
  return error
}

function fake(upload: MeetingUpload): FakeApi {
  const api: FakeApi = { upload, sent: [], failures: []}
  vi.mocked(uploadMeetingPart).mockImplementation(async (_id, kind, index, part) => {
    const failure = api.failures.shift()
    if (failure) {
      throw failure
    }
    const file = api.upload.files.find((stored) => stored.kind === kind)!
    const next = Math.floor(file.receivedBytes / PART_BYTES)
    if (index > next) {
      throw refusal(409, { code: 'meeting_upload_part_ahead' })
    }
    const text = await part.text()
    api.sent.push({ kind, index, text })
    if (index === next) {
      file.receivedBytes += part.size
    }
    return { upload: structuredClone(api.upload) }
  })
  vi.mocked(getMeetingUpload).mockImplementation(async () => ({ upload: structuredClone(api.upload) }))
  return api
}

const noWait = vi.fn<(delayMs: number, signal: AbortSignal) => Promise<void>>(async () => {})

beforeEach(() => {
  vi.mocked(uploadMeetingPart).mockReset()
  vi.mocked(getMeetingUpload).mockReset()
  noWait.mockClear()
})

describe('sendMeetingFiles', () => {
  it('sends the transcript first, then every part of the recording the upload lacks, in order', async () => {
    // Two parts of the recording arrived before; the transcript has not.
    const api = fake(startUpload(10, 8, 3))
    const progress: number[] = []

    const sent = await sendMeetingFiles({
      upload: startUpload(10, 8, 3),
      files: {
        recording: new File([ 'abcdefghij' ], 'recording.mp4'),
        transcript: new File([ 'Hi.' ], 'transcript.txt'),
      },
      signal: new AbortController().signal,
      onProgress: (moved) => progress.push(moved.files.reduce((sum, file) => sum + file.receivedBytes, 0)),
      wait: noWait,
    })

    expect(api.sent).toEqual([
      { kind: 'transcript', index: 0, text: 'Hi.' },
      // Each part a slice of the file, the last one shorter.
      { kind: 'recording', index: 2, text: 'ij' },
    ])
    expect(progress).toEqual([ 11, 13 ])
    expect(sent.files.every((file) => file.receivedBytes === file.sizeBytes)).toBe(true)
  })

  it('sends a file attached to a meeting in parts, alone in its upload, as a recording is sent', async () => {
    const attaching = {
      ...startUpload(1),
      meetingId: 'meeting',
      files: [{
        kind: 'attachment' as const,
        fileName: 'Roadmap.pdf',
        sizeBytes: 6,
        modifiedAt: null,
        receivedBytes: 0,
      }],
    }
    const api = fake(structuredClone(attaching))

    const sent = await sendMeetingFiles({
      upload: attaching,
      files: { attachment: new File([ 'abcdef' ], 'Roadmap.pdf') },
      signal: new AbortController().signal,
      onProgress: () => {},
      wait: noWait,
    })

    expect(api.sent).toEqual([
      { kind: 'attachment', index: 0, text: 'abcd' },
      { kind: 'attachment', index: 1, text: 'ef' },
    ])
    expect(sent.files[0].receivedBytes).toBe(6)
  })

  it('waits out a dropped connection or a busy API, and sends the same part again', async () => {
    const api = fake(startUpload(8))
    api.failures.push(
      new NetworkError(new Request('http://localhost/')),
      refusal(429, { code: 'too_many_requests' }, { 'Retry-After': '7' }),
    )

    await sendMeetingFiles({
      upload: startUpload(8),
      files: { recording: new File([ 'abcdefgh' ], 'recording.mp4'), transcript: null },
      signal: new AbortController().signal,
      onProgress: () => {},
      wait: noWait,
    })

    expect(api.sent.map((part) => part.index)).toEqual([ 0, 1 ])
    expect(noWait).toHaveBeenCalledTimes(2)
    // The second wait is as long as the API asked.
    expect(noWait).toHaveBeenLastCalledWith(7_000, expect.any(AbortSignal))
  })

  it('carries on from where the API is when a part was ahead of it', async () => {
    // The page read the upload as further along than the API has it.
    const api = fake(startUpload(8))

    await sendMeetingFiles({
      upload: startUpload(8, 4),
      files: { recording: new File([ 'abcdefgh' ], 'recording.mp4'), transcript: null },
      signal: new AbortController().signal,
      onProgress: () => {},
      wait: noWait,
    })

    expect(getMeetingUpload).toHaveBeenCalledTimes(1)
    expect(api.sent).toEqual([
      { kind: 'recording', index: 0, text: 'abcd' },
      { kind: 'recording', index: 1, text: 'efgh' },
    ])
  })

  it('acknowledges a part the API already has without counting it twice', async () => {
    // Another tab sent the first part meanwhile.
    const api = fake(startUpload(8, 4))

    await sendMeetingFiles({
      upload: startUpload(8),
      files: { recording: new File([ 'abcdefgh' ], 'recording.mp4'), transcript: null },
      signal: new AbortController().signal,
      onProgress: () => {},
      wait: noWait,
    })

    expect(api.sent.map((part) => part.index)).toEqual([ 0, 1 ])
    expect(api.upload.files[0].receivedBytes).toBe(8)
  })

  it('stops at a refusal, and after as many attempts as a part gets', async () => {
    const refused = fake(startUpload(4))
    refused.failures.push(refusal(400, { code: 'meeting_upload_not_media' }))
    const options = {
      upload: startUpload(4),
      files: { recording: new File([ 'abcd' ], 'recording.mp4'), transcript: null },
      signal: new AbortController().signal,
      onProgress: () => {},
      wait: noWait,
    }

    await expect(sendMeetingFiles(options)).rejects.toBeInstanceOf(HTTPError)
    expect(noWait).not.toHaveBeenCalled()

    const away = fake(startUpload(4))
    for (let attempt = 0; attempt < MEETING_UPLOAD_PART_ATTEMPTS; attempt++) {
      away.failures.push(refusal(503, null))
    }
    await expect(sendMeetingFiles(options)).rejects.toBeInstanceOf(HTTPError)
    expect(noWait).toHaveBeenCalledTimes(MEETING_UPLOAD_PART_ATTEMPTS - 1)
    expect(away.sent).toEqual([])
  })

  it('stops once the page closes', async () => {
    fake(startUpload(8))
    const closing = new AbortController()
    closing.abort()

    await expect(sendMeetingFiles({
      upload: startUpload(8),
      files: { recording: new File([ 'abcdefgh' ], 'recording.mp4'), transcript: null },
      signal: closing.signal,
      onProgress: () => {},
      wait: noWait,
    })).rejects.toThrow()
    expect(uploadMeetingPart).not.toHaveBeenCalled()
  })
})
