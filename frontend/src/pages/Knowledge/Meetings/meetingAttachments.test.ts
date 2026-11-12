// Copyright © 2026 Jalapeno Labs

import type { MeetingAttachment, MeetingMark, MeetingWord } from '../../../api/routes/meetingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  attachmentIdOfPin,
  firstPinOf,
  orderAttachments,
  pinnedOver,
  placePins,
  refusalOf,
} from './meetingAttachments'

function word(id: string, position: number): MeetingWord {
  return {
    id,
    position,
    text: id,
    speaker: 'Ada',
    startMs: position * 1000,
    endMs: position * 1000 + 900,
    origin: 'source',
    captureId: null,
  }
}

function mark(id: string, kind: string, start: string, end: string, data: Record<string, unknown> = {}): MeetingMark {
  return {
    id,
    kind,
    startWordId: start,
    endWordId: end,
    data,
    createdBy: 'ada',
    agent: null,
    createdAt: '2026-11-12T10:00:00Z',
    excerpt: null,
  }
}

function attachment(id: string, kind: 'file' | 'frame', meetingMs: number | null = null): MeetingAttachment {
  return {
    id,
    kind,
    fileName: `${id}.webp`,
    contentType: 'image/webp',
    sizeBytes: 100,
    address: `/api/v1/meetings/m/attachments/${id}/${id}.webp`,
    displaysInline: kind === 'frame',
    caption: kind === 'frame'
      ? `What ${id} shows`
      : null,
    captureId: null,
    captureMs: meetingMs,
    meetingMs,
    textState: 'none',
    createdBy: 'ada',
    agent: null,
    createdAt: '2026-11-12T10:00:00Z',
  }
}

const words = [ 'w0', 'w1', 'w2', 'w3', 'w4', 'w5' ].map((id, index) => word(id, index))

describe('placePins', () => {
  it('places each pin by the words it lies over, and leaves out every other mark', () => {
    const marks = [
      mark('pin-deck', 'pin', 'w1', 'w2', { attachmentId: 'deck' }),
      mark('excerpt', 'excerpt', 'w0', 'w5'),
      mark('pin-gone', 'pin', 'replaced', 'w3', { attachmentId: 'frame' }),
      mark('pin-bad', 'pin', 'w0', 'w0', { attachmentId: 7 }),
    ]

    expect(placePins(words, marks)).toEqual([
      { attachmentId: 'deck', markId: 'pin-deck', range: { start: 1, end: 2 }},
    ])
    expect(attachmentIdOfPin(marks[1])).toBeNull()
  })
})

describe('pinnedOver', () => {
  it('names what is pinned over any word of a range once, in the order it arrived', () => {
    const pins = placePins(words, [
      mark('a', 'pin', 'w1', 'w1', { attachmentId: 'slide' }),
      mark('b', 'pin', 'w4', 'w5', { attachmentId: 'deck' }),
      mark('c', 'pin', 'w2', 'w4', { attachmentId: 'deck' }),
    ])
    const attachments = [ attachment('deck', 'file'), attachment('slide', 'frame', 1000), attachment('other', 'file') ]

    expect(pinnedOver(pins, attachments, { start: 0, end: 2 }).map((pinned) => pinned.id))
      .toEqual([ 'deck', 'slide' ])
    expect(pinnedOver(pins, attachments, { start: 3, end: 3 }).map((pinned) => pinned.id))
      .toEqual([ 'deck' ])
    expect(pinnedOver(pins, attachments, { start: 0, end: 0 })).toEqual([])
    expect(firstPinOf(pins, 'deck')?.range).toEqual({ start: 2, end: 4 })
    expect(firstPinOf(pins, 'other')).toBeNull()
  })
})

describe('orderAttachments', () => {
  it('lists frames in the order they were shown, then files in the order they arrived', () => {
    const ordered = orderAttachments([
      attachment('late', 'frame', 9000),
      attachment('deck', 'file'),
      attachment('early', 'frame', 2000),
      attachment('spec', 'file'),
    ])

    expect(ordered.frames.map((frame) => frame.id)).toEqual([ 'early', 'late' ])
    expect(ordered.files.map((file) => file.id)).toEqual([ 'deck', 'spec' ])
  })
})

describe('refusalOf', () => {
  it('refuses a file the meeting would refuse before a byte of it is sent', () => {
    const policy = { maxBytes: 10, destinationId: 'files' }

    expect(refusalOf({ size: 10 }, policy)).toBeNull()
    expect(refusalOf({ size: 11 }, policy)).toBe('tooLarge')
    expect(refusalOf({ size: 0 }, policy)).toBe('empty')
    expect(refusalOf({ size: 5 }, { ...policy, destinationId: null })).toBe('nowhere')
  })
})
