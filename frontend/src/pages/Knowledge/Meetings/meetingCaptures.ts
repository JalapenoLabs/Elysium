// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { CaptureOffsetSource, MeetingCapture } from '../../../api/routes/meetingRoutes'

// Misc
import { CAPTURE_OFFSET_SOURCES } from '../../../api/routes/meetingRoutes'

// How a meeting's page names its captures ("recordings" to the person, whether or not one kept
// audio): where each starts on the meeting's clock, how that was found, and which came in from
// another meeting by a merge (docs/meetings.md, Aligning, and The same meeting twice).

export const offsetSourceLabelKeys = {
  listed: 'review.captures.found.listed',
  audio: 'review.captures.found.audio',
  words: 'review.captures.found.words',
  hand: 'review.captures.found.hand',
} as const satisfies Record<CaptureOffsetSource, ParseKeys<'meetings'>>

// How a capture's offset was found, when this build knows the name the API sent.
export function offsetSourceOf(capture: MeetingCapture): CaptureOffsetSource | null {
  const known = CAPTURE_OFFSET_SOURCES.find((source) => source === capture.offsetSource)
  if (!known) {
    console.debug('offsetSourceOf read an offset source it does not know', {
      captureId: capture.id,
      offsetSource: capture.offsetSource,
    })
    return null
  }
  return known
}

// Where a capture starts on the meeting's clock, as a sentence's key and the span it names: with
// the meeting, some time into it, or some time before it (a recording started before the one the
// meeting's clock counts from).
export function startOf(offsetMs: number) {
  if (offsetMs > 0) {
    return { key: 'review.captures.startsInto', spanMs: offsetMs } as const
  }
  if (offsetMs < 0) {
    return { key: 'review.captures.startsBefore', spanMs: -offsetMs } as const
  }
  return { key: 'review.captures.startsWith', spanMs: 0 } as const
}

// The captures of one meeting a merge brought in, or the meeting's own (`from` null).
export type CaptureGroup = {
  from: MeetingCapture['mergedFrom']
  captures: MeetingCapture[]
  // Whether splitting it out can go ahead: a capture of it moved by hand since the merge is the
  // person's change, which splitting would overwrite, so the API refuses it until that move is
  // undone (`captures_sit` in api/src/meetings/duplicates.rs).
  canSplit: boolean
}

// A meeting's captures as its page lists them: its own first, then those of each meeting merged
// in, in the order they joined, each group with what splitting it needs.
export function groupCaptures(captures: MeetingCapture[]): CaptureGroup[] {
  const own: CaptureGroup = { from: null, captures: [], canSplit: false }
  const mergedIn = new Map<string, CaptureGroup>()
  for (const capture of captures) {
    if (!capture.mergedFrom) {
      own.captures.push(capture)
      continue
    }
    const group = mergedIn.get(capture.mergedFrom.meetingId) ?? {
      from: capture.mergedFrom,
      captures: [],
      canSplit: true,
    }
    group.captures.push(capture)
    if (capture.offsetSource === 'hand') {
      group.canSplit = false
    }
    mergedIn.set(capture.mergedFrom.meetingId, group)
  }

  const groups = [ ...mergedIn.values() ]
  if (own.captures.length) {
    groups.unshift(own)
  }
  return groups
}
