// Copyright © 2026 Jalapeno Labs

import type { MeetingCapture, MeetingWord } from '../../../api/routes/meetingRoutes'

// The pure half of a meeting's player (docs/meeting-review.md, The player): which capture it
// starts on, which word is being spoken, and where it floats. Every time here is on the
// meeting's clock unless it says otherwise; a capture's own clock sits `offsetMs` along it.

// Where the player floats once the page scrolls past it, as fractions of the room the window
// leaves around it: 0 is the left or top edge and 1 the right or bottom, so it keeps its corner
// as the window changes size.
export type PlayerPlacement = {
  x: number
  y: number
}

// The bottom right corner, out of the way of the transcript's start.
export const DEFAULT_PLAYER_PLACEMENT: PlayerPlacement = { x: 1, y: 1 }

// How far the left and right arrows jump, back and forward.
export const PLAYER_JUMP_MS = 10_000

// How far from the window's edges a floating player stays, in pixels.
export const PLAYER_EDGE_PX = 16

export function isVideo(contentType: string | null) {
  return Boolean(contentType?.toLowerCase().startsWith('video/'))
}

// The captures the player can play, first first, and the one it starts on: the first with a
// video, since a picture says who is speaking, else the first with a recording at all. Elysium
// keeps no resolution of a recording, so among videos the first is the best it can tell.
export function playableCapturesOf(captures: MeetingCapture[]) {
  const playable: MeetingCapture[] = []
  let firstVideo: MeetingCapture | null = null
  for (const capture of captures) {
    if (!capture.hasRecording) {
      continue
    }
    playable.push(capture)
    if (!firstVideo && isVideo(capture.recordingContentType)) {
      firstVideo = capture
    }
  }
  return {
    playable,
    preferred: firstVideo ?? playable[0] ?? null,
  } as const
}

// The word being spoken at `meetingMs`: the last that started by then, while it still lasts.
// Between two words (a pause, or a line timed with no words in it) nothing is spoken. Words
// stand in the transcript's order, which is the order they were said, so this is a binary search.
export function spokenWordAt(words: MeetingWord[], meetingMs: number): number | null {
  let low = 0
  let high = words.length - 1
  let found = -1
  while (low <= high) {
    const middle = (low + high) >> 1
    if (words[middle].startMs <= meetingMs) {
      found = middle
      low = middle + 1
    }
    else {
      high = middle - 1
    }
  }
  if (found < 0 || meetingMs > words[found].endMs) {
    return null
  }
  return found
}

// Where a moment of the meeting falls in a capture's own recording, in seconds, which is what a
// media element seeks by. A moment before the capture began is its start.
export function captureSecondsAt(meetingMs: number, offsetMs: number) {
  return Math.max(0, meetingMs - offsetMs) / 1000
}

// The moment of the meeting a capture's recording is at, from its own position in seconds.
export function meetingMsAt(captureSeconds: number, offsetMs: number) {
  return Math.round(captureSeconds * 1000) + offsetMs
}

type Room = {
  // The window's size, and the floating player's.
  viewportWidth: number
  viewportHeight: number
  width: number
  height: number
}

function roomOf(room: Room) {
  return {
    across: Math.max(0, room.viewportWidth - room.width - PLAYER_EDGE_PX * 2),
    down: Math.max(0, room.viewportHeight - room.height - PLAYER_EDGE_PX * 2),
  } as const
}

function clampFraction(value: number) {
  if (!Number.isFinite(value)) {
    return 1
  }
  return Math.min(1, Math.max(0, value))
}

// The floating player's top left corner in pixels, for a placement in the room there is.
export function pixelsOf(placement: PlayerPlacement, room: Room) {
  const { across, down } = roomOf(room)
  return {
    left: PLAYER_EDGE_PX + clampFraction(placement.x) * across,
    top: PLAYER_EDGE_PX + clampFraction(placement.y) * down,
  } as const
}

// The placement of a player dragged to `left` and `top`, kept inside the window.
export function placementAt(left: number, top: number, room: Room): PlayerPlacement {
  const { across, down } = roomOf(room)
  return {
    x: across
      ? clampFraction((left - PLAYER_EDGE_PX) / across)
      : 1,
    y: down
      ? clampFraction((top - PLAYER_EDGE_PX) / down)
      : 1,
  }
}
