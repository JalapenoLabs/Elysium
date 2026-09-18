// Copyright © 2026 Jalapeno Labs

import type { BurnupPoint } from '../../api/routes/initiativeRoutes'
import type { ChartBox } from './burnup'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { createBurnupScales, getCountTicks, getNearestPointIndex, getStepPath } from './burnup'

// 100 by 100 with no padding, so positions read as percentages.
const BOX: ChartBox = {
  width: 100,
  height: 100,
  padding: { top: 0, right: 0, bottom: 0, left: 0 },
}

const POINTS: BurnupPoint[] = [
  { at: '2026-09-01T00:00:00.000Z', resolved: 0, total: 0 },
  { at: '2026-09-02T00:00:00.000Z', resolved: 0, total: 4 },
  { at: '2026-09-03T00:00:00.000Z', resolved: 2, total: 4 },
  { at: '2026-09-05T00:00:00.000Z', resolved: 2, total: 4 },
]

describe('getCountTicks', () => {
  it('counts in whole steps from zero past the highest value', () => {
    expect(getCountTicks(4)).toEqual([ 0, 1, 2, 3, 4 ])
    expect(getCountTicks(10)).toEqual([ 0, 3, 6, 9, 12 ])
  })

  it('still draws an axis for an initiative with nothing in it', () => {
    expect(getCountTicks(0)).toEqual([ 0, 1 ])
  })
})

describe('getStepPath', () => {
  it('holds each count flat until the next point, then steps to it', () => {
    const scales = createBurnupScales(POINTS, BOX)

    expect(getStepPath(POINTS, scales, 'total')).toBe('M0,100 H25 V0 H50 V0 H100 V0')
    expect(getStepPath(POINTS, scales, 'resolved')).toBe('M0,100 H25 V100 H50 V50 H100 V50')
  })

  it('spans the box for a burnup of a single moment', () => {
    const single = [ POINTS[0] ]
    const scales = createBurnupScales(single, BOX)

    expect(scales.x(scales.start)).toBe(0)
    expect(getStepPath(single, scales, 'total')).toBe('M0,100')
  })
})

describe('getNearestPointIndex', () => {
  it('snaps to the point closest in time', () => {
    expect(getNearestPointIndex(POINTS, Date.parse('2026-09-02T11:00:00.000Z'))).toBe(1)
    expect(getNearestPointIndex(POINTS, Date.parse('2026-09-04T13:00:00.000Z'))).toBe(3)
    expect(getNearestPointIndex(POINTS, Date.parse('2026-08-01T00:00:00.000Z'))).toBe(0)
  })
})

describe('createBurnupScales', () => {
  it('maps positions back to times within the drawn range', () => {
    const scales = createBurnupScales(POINTS, BOX)

    expect(scales.timeAt(-10)).toBe(scales.start)
    expect(scales.timeAt(50)).toBe(Date.parse('2026-09-03T00:00:00.000Z'))
    expect(scales.timeAt(500)).toBe(scales.end)
  })
})
