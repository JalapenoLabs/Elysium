// Copyright © 2026 Jalapeno Labs

import type { MeetingUpload, MeetingUploadPolicy } from '../../api/routes/meetingUploadRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  describeFileRefusal,
  extensionOf,
  findResumable,
  nextPart,
  partCount,
  retryDelay,
  titleFromFileName,
  uploadProgress,
} from './meetingUpload'

const policy: MeetingUploadPolicy = {
  partBytes: 8,
  recording: { maxBytes: 100, extensions: [ 'mp3', 'wav', 'mp4', 'mov' ]},
  transcript: { maxBytes: 20, extensions: [ 'vtt', 'srt', 'txt', 'md' ]},
  defaultStorageLocationId: null,
}

const CHANGED_AT = Date.parse('2026-11-09T16:00:00Z')

function file(name: string, size: number, lastModified = CHANGED_AT) {
  return new File([ 'x'.repeat(size) ], name, { lastModified })
}

function upload(overrides: Partial<MeetingUpload> = {}): MeetingUpload {
  return {
    id: 'upload-1',
    meetingId: null,
    title: 'Board call',
    startedAt: '2026-11-09T15:00:00Z',
    storageLocationId: 'location-1',
    partBytes: 8,
    files: [
      {
        kind: 'recording',
        fileName: 'Board call.mp4',
        sizeBytes: 20,
        modifiedAt: '2026-11-09T16:00:00Z',
        receivedBytes: 8,
      },
    ],
    captureId: null,
    attachmentId: null,
    finishedAt: null,
    createdAt: '2026-11-09T16:01:00Z',
    updatedAt: '2026-11-09T16:02:00Z',
    ...overrides,
  }
}

const details = {
  meetingId: null,
  title: 'Board call',
  startedAt: '2026-11-09T15:00:00.000Z',
  storageLocationId: 'location-1',
}

describe('describeFileRefusal', () => {
  it('takes a file of a kind the meeting takes, within its size', () => {
    expect(describeFileRefusal(file('call.MP4', 100), 'recording', policy)).toBeNull()
    expect(describeFileRefusal(file('notes.md', 5), 'transcript', policy)).toBeNull()
  })

  it('refuses another type, an empty file, and one past the limit', () => {
    expect(describeFileRefusal(file('call.webm', 5), 'recording', policy)).toBe('type')
    expect(describeFileRefusal(file('notes.vtt', 5), 'recording', policy)).toBe('type')
    expect(describeFileRefusal(file('notes.vtt', 0), 'transcript', policy)).toBe('empty')
    expect(describeFileRefusal(file('call.mp3', 101), 'recording', policy)).toBe('tooLarge')
  })
})

describe('extensionOf', () => {
  it('reads the last extension in lowercase, and none from a name without one', () => {
    expect(extensionOf('Weekly.sync.MOV')).toBe('mov')
    expect(extensionOf('README')).toBe('')
  })
})

describe('titleFromFileName', () => {
  it('names a meeting from its file, as a recorder wrote it', () => {
    expect(titleFromFileName('2026-11-09_Weekly__sync.mp4')).toBe('2026-11-09 Weekly sync')
    expect(titleFromFileName('Board call.vtt')).toBe('Board call')
    expect(titleFromFileName('.hidden')).toBe('.hidden')
  })
})

describe('findResumable', () => {
  it('finds the upload these same files started, as the same meeting', () => {
    const started = upload()
    const files = { recording: file('Board  call.mp4', 20), transcript: null }

    expect(findResumable([ started ], details, files)).toBe(started)
  })

  it('starts again for other files, other details, or another meeting', () => {
    const started = upload()
    const same = { recording: file('Board call.mp4', 20), transcript: null }
    const cases = [
      // Another file of the same name and size, changed at another time.
      [ details, { recording: file('Board call.mp4', 20, CHANGED_AT + 1), transcript: null }],
      // A transcript beside it this time.
      [ details, { recording: file('Board call.mp4', 20), transcript: file('notes.vtt', 3) }],
      [{ ...details, title: 'Board call, again' }, same ],
      [{ ...details, startedAt: '2026-11-09T15:01:00Z' }, same ],
      [{ ...details, storageLocationId: 'location-2' }, same ],
      [{ ...details, meetingId: 'meeting-1', title: null, startedAt: null }, same ],
    ] as const

    for (const [ asked, files ] of cases) {
      expect(findResumable([ started ], asked, files)).toBeNull()
    }
  })
})

describe('parts', () => {
  it('counts a file in whole parts and a shorter last one, and resumes after what arrived', () => {
    expect(partCount(20, 8)).toBe(3)
    expect(partCount(16, 8)).toBe(2)
    const [ recording ] = upload().files
    expect(nextPart(recording, 8)).toBe(1)
    // A file all there, its last part shorter than the rest, needs no part.
    expect(nextPart({ ...recording, receivedBytes: 20 }, 8)).toBe(3)
    expect(uploadProgress(upload())).toEqual({ received: 8, total: 20 })
  })

  it('waits longer each time, up to a most, and as long as the API asks', () => {
    expect([ 1, 2, 3, 4 ].map((attempt) => retryDelay(attempt, null, 1_000, 5_000)))
      .toEqual([ 1_000, 2_000, 4_000, 5_000 ])
    expect(retryDelay(1, 9, 1_000, 5_000)).toBe(9_000)
    expect(retryDelay(4, 1, 1_000, 5_000)).toBe(5_000)
  })
})
