// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { toDesktopPoint, toScrollSteps } from './pointer'

const desktop = { width: 1920, height: 1080 }

describe('toDesktopPoint', () => {
  it('scales a point on an element the desktop fills exactly', () => {
    expect(toDesktopPoint({ x: 480, y: 270, width: 960, height: 540 }, desktop)).toEqual({ x: 960, y: 540 })
  })

  it('accounts for the bars above and below a letterboxed picture', () => {
    // A 960 wide picture is 540 tall; in a 740 tall element it sits 100 pixels down.
    expect(toDesktopPoint({ x: 0, y: 100, width: 960, height: 740 }, desktop)).toEqual({ x: 0, y: 0 })
    expect(toDesktopPoint({ x: 960, y: 640, width: 960, height: 740 }, desktop)).toEqual({ x: 1919, y: 1079 })
  })

  it('accounts for the bars beside a pillarboxed picture', () => {
    // A 540 tall picture is 960 wide; in a 1160 wide element it starts 100 pixels in.
    expect(toDesktopPoint({ x: 580, y: 270, width: 1160, height: 540 }, desktop)).toEqual({ x: 960, y: 540 })
  })

  it('pins a point in the bars to the nearest edge', () => {
    expect(toDesktopPoint({ x: 10, y: 5, width: 960, height: 740 }, desktop)).toEqual({ x: 20, y: 0 })
  })
})

describe('toScrollSteps', () => {
  it('turns wheel travel into whole steps in the same direction', () => {
    expect(toScrollSteps(120)).toBe(3)
    expect(toScrollSteps(-80)).toBe(-2)
    expect(toScrollSteps(0)).toBe(0)
  })

  it('never drops a small movement entirely', () => {
    expect(toScrollSteps(4)).toBe(1)
    expect(toScrollSteps(-4)).toBe(-1)
  })
})
