// Copyright © 2026 Jalapeno Labs

import type { BurnupPoint } from '../../api/routes/initiativeRoutes'

// Geometry for the burnup chart, kept apart from the component so it can be tested. The API
// sends a point at creation, one at every moment either count changed, and one now; between
// points nothing changed, so both series are drawn as steps: flat until the next point,
// then straight up or down to it.

export type ChartBox = {
  width: number
  height: number
  // Room inside the box for axis labels.
  padding: { top: number, right: number, bottom: number, left: number }
}

// A y-axis with at most this many intervals, on whole numbers, since items are counted.
const MAX_Y_INTERVALS = 4

// Whole-number ticks from zero to at least `max`, evenly spaced.
export function getCountTicks(max: number) {
  const top = Math.max(max, 1)
  const step = Math.max(1, Math.ceil(top / MAX_Y_INTERVALS))
  const ticks: number[] = []
  for (let tick = 0; tick < top + step; tick += step) {
    ticks.push(tick)
  }
  return ticks
}

// Screen positions for times and counts inside the box.
export function createBurnupScales(points: BurnupPoint[], box: ChartBox) {
  const times = points.map((point) => Date.parse(point.at))
  const start = Math.min(...times)
  // A burnup with a single moment still spans the box.
  const end = Math.max(Math.max(...times), start + 1)
  let highest = 0
  for (const point of points) {
    highest = Math.max(highest, point.total, point.resolved)
  }
  const ticks = getCountTicks(highest)
  const top = ticks[ticks.length - 1]

  const left = box.padding.left
  const right = box.width - box.padding.right
  const bottom = box.height - box.padding.bottom

  return {
    start,
    end,
    ticks,
    x: (time: number) => left + ((time - start) / (end - start)) * (right - left),
    y: (count: number) => bottom - (count / top) * (bottom - box.padding.top),
    // The time under a horizontal position, clamped to the drawn range.
    timeAt: (position: number) => {
      const ratio = Math.min(Math.max((position - left) / (right - left), 0), 1)
      return start + ratio * (end - start)
    },
  } as const
}

export type BurnupScales = ReturnType<typeof createBurnupScales>

// An SVG path drawing one series as steps.
export function getStepPath(
  points: BurnupPoint[],
  scales: BurnupScales,
  series: 'resolved' | 'total',
) {
  const commands: string[] = []
  for (const [ index, point ] of points.entries()) {
    const x = scales.x(Date.parse(point.at))
    const y = scales.y(point[series])
    if (index === 0) {
      commands.push(`M${x},${y}`)
      continue
    }
    commands.push(`H${x}`, `V${y}`)
  }
  return commands.join(' ')
}

// The index of the point closest in time to `time`, where the chart's crosshair snaps.
export function getNearestPointIndex(points: BurnupPoint[], time: number) {
  let nearestIndex = 0
  let nearestDistance = Number.POSITIVE_INFINITY
  for (const [ index, point ] of points.entries()) {
    const distance = Math.abs(Date.parse(point.at) - time)
    if (distance < nearestDistance) {
      nearestIndex = index
      nearestDistance = distance
    }
  }
  return nearestIndex
}
