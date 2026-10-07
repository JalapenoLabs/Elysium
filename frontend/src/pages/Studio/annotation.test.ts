// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getArrowHeadPoints, getPenSize, outlineToPathData, toImagePoint } from './annotation'

describe('toImagePoint', () => {
  it('scales a point on the shown canvas to the image\'s own pixels', () => {
    // A 2000 by 1000 image shown at 500 by 250, 100 pixels from the left and 50 from the top.
    const rect = { left: 100, top: 50, width: 500, height: 250 }
    expect(toImagePoint(350, 175, 0.5, rect, 2000, 1000)).toEqual([ 1000, 500, 0.5 ])
    expect(toImagePoint(100, 50, 1, rect, 2000, 1000)).toEqual([ 0, 0, 1 ])
  })
})

describe('getPenSize', () => {
  it('follows the image\'s shorter side', () => {
    expect(getPenSize(1024, 1024)).toBe(8)
    expect(getPenSize(4000, 2000)).toBe(16)
  })

  it('never thins below four pixels', () => {
    expect(getPenSize(200, 100)).toBe(4)
  })
})

describe('getArrowHeadPoints', () => {
  it('sweeps both ends back from the tip, either side of the shaft', () => {
    const [ left, right ] = getArrowHeadPoints([ 0, 0, 0.5 ], [ 100, 0, 0.5 ], 20)
    // Behind the tip along the shaft, and mirrored above and below it.
    expect(left[0]).toBeCloseTo(100 - 20 * Math.cos(Math.PI / 6))
    expect(right[0]).toBeCloseTo(left[0])
    expect(left[1]).toBeCloseTo(10)
    expect(right[1]).toBeCloseTo(-10)
  })

  it('points the head along any direction', () => {
    const [ left, right ] = getArrowHeadPoints([ 0, 0, 0.5 ], [ 0, 100, 0.5 ], 20)
    expect(left[1]).toBeLessThan(100)
    expect(right[1]).toBeLessThan(100)
    expect(left[0]).toBeCloseTo(-right[0])
  })
})

describe('outlineToPathData', () => {
  it('draws nothing for an empty outline', () => {
    expect(outlineToPathData([])).toBe('')
  })

  it('closes a curve through the midpoints of the outline', () => {
    const path = outlineToPathData([[ 0, 0 ], [ 10, 0 ], [ 10, 10 ]])
    expect(path).toBe('M 0 0 Q 0 0 5 0 10 0 10 5 10 10 5 5 Z')
  })
})
