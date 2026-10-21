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

// Whole scroll steps for a wheel event's travel, keeping its direction.
export function toScrollSteps(deltaPixels: number) {
  if (!deltaPixels) {
    return 0
  }
  const steps = Math.round(deltaPixels / PIXELS_PER_SCROLL_STEP)
  if (steps) {
    return steps
  }
  return Math.sign(deltaPixels)
}
