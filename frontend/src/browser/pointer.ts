// Copyright © 2026 Jalapeno Labs

import type { ScreenSize } from './nekoConnection'

// Where a point on the stream's element lands on the remote desktop.
//
// The video keeps the desktop's aspect ratio inside its element (`object-fit: contain`), so
// it is letterboxed on two sides. A point in the bars maps to the nearest edge, so a drag
// that leaves the picture still ends where the pointer last was on it.

export type ElementPoint = {
  // The offset into the element, in CSS pixels.
  x: number
  y: number
  // The element's own size, in CSS pixels.
  width: number
  height: number
}

export function toDesktopPoint(point: ElementPoint, screen: ScreenSize) {
  const scale = Math.min(point.width / screen.width, point.height / screen.height)
  const pictureWidth = screen.width * scale
  const pictureHeight = screen.height * scale
  const left = (point.width - pictureWidth) / 2
  const top = (point.height - pictureHeight) / 2

  const x = Math.round((point.x - left) / scale)
  const y = Math.round((point.y - top) / scale)
  return {
    x: Math.min(Math.max(x, 0), screen.width - 1),
    y: Math.min(Math.max(y, 0), screen.height - 1),
  } as const
}

// One scroll step per this many pixels of wheel travel, so a trackpad's many small events
// and a mouse wheel's few large ones scroll the remote page alike.
const PIXELS_PER_SCROLL_STEP = 40

// How far one unit of a wheel event's `deltaMode` travels, in pixels. A line is one step,
// as a wheel that reports lines (Firefox on Windows) moves three per notch, as far as a
// wheel that reports 120 pixels. Pages are rare (a wheel set to scroll a page per notch);
// one counts as ten steps.
const pixelsByDeltaMode: Record<number, number> = {
  0: 1,
  1: PIXELS_PER_SCROLL_STEP,
  2: PIXELS_PER_SCROLL_STEP * 10,
}

export type ScrollSteps = {
  steps: number
  // Travel short of a whole step, in pixels, kept for the next event.
  carry: number
}

// Whole scroll steps for a wheel event's travel on one axis, adding what earlier events left
// over. Each event on its own is often short of a step, and rounding each up would scroll
// a trackpad many times further than it moved, so the remainder carries forward instead.
export function toScrollSteps(delta: number, deltaMode: number, carry: number): ScrollSteps {
  const pixelsPerUnit = pixelsByDeltaMode[deltaMode]
  if (pixelsPerUnit === undefined) {
    console.debug('toScrollSteps ignored a wheel event of an unknown delta mode', { deltaMode })
    return { steps: 0, carry }
  }
  const travel = carry + delta * pixelsPerUnit
  // `Math.trunc` keeps the sign of a negative fraction as -0; no steps is plain 0.
  const steps = Math.trunc(travel / PIXELS_PER_SCROLL_STEP) || 0
  return {
    steps,
    carry: travel - steps * PIXELS_PER_SCROLL_STEP,
  }
}
