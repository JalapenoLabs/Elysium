// Copyright © 2026 Jalapeno Labs

import type { ThreadStatus } from '../../api/routes/codingSessionRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeCodingSession, makeStudioItem } from '../../testFixtures'
import {
  filterStudioItems,
  getTileThreadState,
  getTileThumbnail,
  NO_PROJECT,
  readStudioFilters,
  writeStudioFilters,
} from './studioListing'

function makeThread(overrides: Partial<ThreadStatus>): ThreadStatus {
  return {
    state: 'idle',
    queueDepth: 0,
    currentTurnId: null,
    latestSequence: 0,
    lastActivityAt: null,
    expiresAt: null,
    ...overrides,
  }
}

describe('readStudioFilters and writeStudioFilters', () => {
  it('reads an empty address as every live item', () => {
    expect(readStudioFilters(new URLSearchParams())).toEqual({ project: null, deleted: false })
  })

  it('writes the defaults as an empty address', () => {
    expect(writeStudioFilters({ project: null, deleted: false }).toString()).toBe('')
  })

  it('round-trips a project and the deleted view', () => {
    const filters = { project: 'banana', deleted: true }
    expect(readStudioFilters(writeStudioFilters(filters))).toEqual(filters)
  })

  it('treats anything but true as the live view', () => {
    expect(readStudioFilters(new URLSearchParams('deleted=yes')).deleted).toBe(false)
  })
})

describe('filterStudioItems', () => {
  const inBanana = makeStudioItem({ id: 'in-banana', projectId: 'banana' })
  const inApple = makeStudioItem({ id: 'in-apple', projectId: 'apple' })
  const inNone = makeStudioItem({ id: 'in-none', projectId: null })
  const items = [ inBanana, inApple, inNone ]

  it('keeps every item, in order, with no project chosen', () => {
    expect(filterStudioItems(items, { project: null, deleted: false })).toEqual(items)
  })

  it('keeps the items of the chosen project', () => {
    expect(filterStudioItems(items, { project: 'banana', deleted: false })).toEqual([ inBanana ])
  })

  it('keeps the items in no project', () => {
    expect(filterStudioItems(items, { project: NO_PROJECT, deleted: false })).toEqual([ inNone ])
  })
})

describe('getTileThumbnail', () => {
  const session = makeCodingSession({ id: 1, studioItemId: 'item' })

  it('shows the image the API chose', () => {
    const item = makeStudioItem({ id: 'item', thumbnailAssetId: 'asset' })
    expect(getTileThumbnail(item, undefined)).toEqual({ kind: 'image', assetId: 'asset' })
  })

  it('shows the working placeholder while a just-started thread has not been polled', () => {
    const item = makeStudioItem({ id: 'item' })
    expect(getTileThumbnail(item, session)).toEqual({ kind: 'working' })
  })

  it('shows the working placeholder while a turn runs or waits', () => {
    const item = makeStudioItem({ id: 'item' })
    const running = { ...session, thread: makeThread({ state: 'running' }) }
    const queued = { ...session, thread: makeThread({ state: 'idle', queueDepth: 1 }) }
    expect(getTileThumbnail(item, running)).toEqual({ kind: 'working' })
    expect(getTileThumbnail(item, queued)).toEqual({ kind: 'working' })
  })

  it('shows an empty frame when nothing is running', () => {
    const item = makeStudioItem({ id: 'item' })
    const idle = { ...session, thread: makeThread({ state: 'idle' }) }
    const expired = { ...session, thread: makeThread({ state: 'expired', queueDepth: 2 }) }
    expect(getTileThumbnail(item, idle)).toEqual({ kind: 'empty' })
    expect(getTileThumbnail(item, expired)).toEqual({ kind: 'empty' })
    expect(getTileThumbnail(item, undefined)).toEqual({ kind: 'empty' })
  })
})

describe('getTileThreadState', () => {
  it('shows no chip for an item with no session', () => {
    expect(getTileThreadState(undefined)).toBeNull()
  })

  it('shows no chip for a thread the API has not polled yet', () => {
    const session = makeCodingSession({ id: 1, studioItemId: 'item', thread: null })

    expect(getTileThreadState(session)).toBeNull()
  })

  it('does not present an unknown state as a state', () => {
    const session = makeCodingSession({ id: 1, studioItemId: 'item', thread: makeThread({ state: 'unknown' }) })

    expect(getTileThreadState(session)).toBeNull()
  })

  it.each([ 'running', 'idle', 'expired' ] as const)('shows a %s thread\'s state', (state) => {
    const session = makeCodingSession({ id: 1, studioItemId: 'item', thread: makeThread({ state }) })

    expect(getTileThreadState(session)).toBe(state)
  })
})
