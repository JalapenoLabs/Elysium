// Copyright © 2026 Jalapeno Labs

import type { ThreadStatus } from '../../api/routes/codingSessionRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeCodingSession } from '../../testFixtures'
import { getStudioThreadStatus } from './studioContinuation'

function makeThread(state: ThreadStatus['state']): ThreadStatus {
  return {
    state,
    queueDepth: 0,
    currentTurnId: null,
    latestSequence: 0,
    lastActivityAt: null,
    expiresAt: null,
  }
}

describe('getStudioThreadStatus', () => {
  it('does not guess before the item\'s sessions have loaded', () => {
    expect(getStudioThreadStatus(undefined, false)).toBe('loading')
  })

  it('reports an item with no session as never run, which has no previous satellite', () => {
    expect(getStudioThreadStatus(undefined, true)).toBe('neverRan')
  })

  it('reports a deleted satellite even while the thread still reads as live', () => {
    const session = makeCodingSession({ id: 1, satelliteId: null, thread: makeThread('idle') })

    expect(getStudioThreadStatus(session, true)).toBe('satelliteDeleted')
  })

  it.each([ 'expired', 'destroyed' ] as const)('reports a %s thread on a known satellite as ended', (state) => {
    const session = makeCodingSession({ id: 1, thread: makeThread(state) })

    expect(getStudioThreadStatus(session, true)).toBe('ended')
  })

  it.each([ 'idle', 'running', 'provisioning', 'unknown' ] as const)('reports a %s thread as live', (state) => {
    const session = makeCodingSession({ id: 1, thread: makeThread(state) })

    expect(getStudioThreadStatus(session, true)).toBe('live')
  })

  it('reports a thread the API has not polled yet as live', () => {
    const session = makeCodingSession({ id: 1, thread: null })

    expect(getStudioThreadStatus(session, true)).toBe('live')
  })
})
