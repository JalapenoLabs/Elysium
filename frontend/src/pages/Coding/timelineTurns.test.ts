// Copyright © 2026 Jalapeno Labs

import type { SessionEvent, SessionEventPayload } from '../../api/routes/codingSessionRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getTurnFallbackLine, groupTimelineTurns } from './timelineTurns'

function event(sequence: number, turnId: string | null, payload: SessionEventPayload | null): SessionEvent {
  return {
    sessionId: 1,
    sequence,
    turnId,
    occurredAt: null,
    type: payload?.kind ?? 'other',
    payload,
  }
}

const message = (text: string): SessionEventPayload => ({ kind: 'agentMessage', text })

describe('groupTimelineTurns', () => {
  it('holds each turn\'s prompt apart from its log, in the order turns started', () => {
    const entries = groupTimelineTurns([
      event(1, 'a', { kind: 'turnStarted', prompt: 'Fix the form' }),
      event(2, 'a', message('On it.')),
      event(3, 'a', { kind: 'turnCompleted', status: 'completed', summary: 'Fixed.', error: null }),
      event(4, 'b', { kind: 'turnStarted', prompt: 'Push it' }),
      event(5, 'b', message('Pushing.')),
    ])

    expect(entries).toHaveLength(2)
    const [ first, second ] = entries
    expect(first.kind === 'turn' && first.prompt?.sequence).toBe(1)
    expect(first.kind === 'turn' && first.log.map((logged) => logged.sequence)).toEqual([ 2, 3 ])
    expect(first.kind === 'turn' && first.completed?.status).toBe('completed')
    expect(second.kind === 'turn' && second.completed).toBeNull()
  })

  it('shows an event that belongs to no turn on its own', () => {
    const entries = groupTimelineTurns([ event(1, null, message('Hello')) ])

    expect(entries).toEqual([{ kind: 'event', event: event(1, null, message('Hello')) }])
  })
})

describe('getTurnFallbackLine', () => {
  it('uses the first line of the agent\'s final message', () => {
    const [ turn ] = groupTimelineTurns([
      event(1, 'a', { kind: 'turnCompleted', status: 'completed', summary: '\nDone.\nDetails', error: null }),
    ])

    expect(turn.kind === 'turn' && getTurnFallbackLine(turn)).toBe('Done.')
  })

  it('has nothing to say for a turn without a final message', () => {
    const [ turn ] = groupTimelineTurns([ event(1, 'a', message('Working')) ])

    expect(turn.kind === 'turn' && getTurnFallbackLine(turn)).toBeNull()
  })
})
