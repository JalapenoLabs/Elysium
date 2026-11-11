// Copyright © 2026 Jalapeno Labs

import type { MeetingCapture, MeetingWord } from '../../../api/routes/meetingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  captureSecondsAt,
  DEFAULT_PLAYER_PLACEMENT,
  meetingMsAt,
  pixelsOf,
  placementAt,
  PLAYER_EDGE_PX,
  playableCapturesOf,
  spokenWordAt,
} from './meetingPlayer'

function word(index: number, startMs: number, endMs: number): MeetingWord {
  return {
    id: `w${index}`,
    position: (index + 1) * 1024,
    text: `word${index}`,
    speaker: 'Ada',
    startMs,
    endMs,
    origin: 'source',
    captureId: 'capture',
  }
}

function capture(id: string, recordingContentType: string | null): MeetingCapture {
  return {
    id,
    connectorId: null,
    sourceKind: 'fireflies',
    url: null,
    offsetMs: 0,
    hasRecording: recordingContentType !== null,
    recordingContentType,
  }
}

describe('spokenWordAt', () => {
  // Two lines, the second starting after a pause of a second.
  const words = [ word(0, 0, 500), word(1, 500, 1000), word(2, 2000, 2400) ]

  it('finds the word whose time holds the moment', () => {
    expect(spokenWordAt(words, 0)).toBe(0)
    expect(spokenWordAt(words, 499)).toBe(0)
    expect(spokenWordAt(words, 500)).toBe(1)
    expect(spokenWordAt(words, 2400)).toBe(2)
  })

  it('finds nothing in a pause, before the first word, or after the last', () => {
    expect(spokenWordAt(words, 1500)).toBeNull()
    expect(spokenWordAt(words, -1)).toBeNull()
    expect(spokenWordAt(words, 5000)).toBeNull()
    expect(spokenWordAt([], 0)).toBeNull()
  })

  it('takes the last of words that share a start, as an edit spreads its words', () => {
    const shared = [ word(0, 0, 0), word(1, 0, 900) ]
    expect(spokenWordAt(shared, 0)).toBe(1)
  })
})

describe('playableCapturesOf', () => {
  it('starts on the first video, over audio listed before it', () => {
    const { playable, preferred } = playableCapturesOf([
      capture('transcript', null),
      capture('audio', 'audio/mpeg'),
      capture('video', 'video/mp4'),
      capture('second', 'video/webm'),
    ])
    expect(playable.map((option) => option.id)).toEqual([ 'audio', 'video', 'second' ])
    expect(preferred?.id).toBe('video')
  })

  it('starts on audio when there is no video, and on nothing without a recording', () => {
    expect(playableCapturesOf([ capture('audio', 'audio/wav') ]).preferred?.id).toBe('audio')
    expect(playableCapturesOf([ capture('transcript', null) ]).preferred).toBeNull()
  })
})

describe('the meeting clock and a capture\'s', () => {
  it('moves a moment by the capture\'s offset, never before its start', () => {
    expect(captureSecondsAt(12_000, 5000)).toBe(7)
    expect(captureSecondsAt(2000, 5000)).toBe(0)
    expect(meetingMsAt(7, 5000)).toBe(12_000)
  })
})

describe('where the player floats', () => {
  const room = { viewportWidth: 1000, viewportHeight: 800, width: 300, height: 200 }

  it('starts in the bottom right corner, clear of the edges', () => {
    expect(pixelsOf(DEFAULT_PLAYER_PLACEMENT, room)).toEqual({
      left: 1000 - 300 - PLAYER_EDGE_PX,
      top: 800 - 200 - PLAYER_EDGE_PX,
    })
  })

  it('keeps its place as a share of the room, inside the window', () => {
    const placed = placementAt(PLAYER_EDGE_PX + 334, PLAYER_EDGE_PX, room)
    expect(placed.x).toBeCloseTo(0.5)
    expect(placed.y).toBe(0)
    expect(placementAt(-500, 5000, room)).toEqual({ x: 0, y: 1 })

    // A narrower window keeps the same share of what room it has.
    const narrow = { ...room, viewportWidth: 600 }
    expect(pixelsOf(placed, narrow).left).toBeCloseTo(PLAYER_EDGE_PX + 134)
  })
})
