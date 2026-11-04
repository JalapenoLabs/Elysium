// Copyright © 2026 Jalapeno Labs

import type { ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { MEETING_STATES } from '../../../api/routes/meetingRoutes'
import { makeConnector } from '../../../testFixtures'
import {
  formatDuration,
  meetingSourcesOf,
  meetingStateChipColors,
  meetingStateLabelKeys,
} from './meetingsPresentation'

describe('formatDuration', () => {
  it('reads like a player, with hours only once there are some', () => {
    expect(formatDuration(0)).toBe('0:00')
    expect(formatDuration(65_400)).toBe('1:05')
    expect(formatDuration(3_725_000)).toBe('1:02:05')
  })

  it('answers nothing for a length the source did not give', () => {
    expect(formatDuration(null)).toBeNull()
    expect(formatDuration(-1)).toBeNull()
  })
})

describe('meeting states', () => {
  it('has a label and a color for every state the API sends', () => {
    for (const state of MEETING_STATES) {
      expect(meetingStateLabelKeys[state]).toBe(`states.${state}`)
      expect(meetingStateChipColors[state]).toBeTruthy()
    }
    expect(meetingStateChipColors.failed).toBe('danger')
    expect(meetingStateChipColors.done).toBe('success')
  })
})

// A kind's definition as the API serves it, providing what the test names.
function makeDefinition(
  kind: ConnectorKindDefinition['kind'],
  provides: ConnectorKindDefinition['provides'],
): ConnectorKindDefinition {
  return { kind, provides, scope: 'personal', account: 'browser', startUrl: null, content: [], checks: []}
}

describe('meetingSourcesOf', () => {
  it('finds sources by what each kind declares it provides, never by name', () => {
    // Teams stands in for a second meeting source: the page must follow the definitions.
    const definitions = [
      makeDefinition('jira', [ 'issues' ]),
      makeDefinition('fireflies', [ 'meetings' ]),
      makeDefinition('teams', [ 'messages', 'meetings' ]),
    ]
    const connectors = [
      makeConnector({ id: 'jira' }),
      makeConnector({ id: 'notes', kind: 'fireflies' }),
      makeConnector({ id: 'chat', kind: 'teams' }),
    ]

    const sources = meetingSourcesOf(definitions, connectors)

    expect(sources.kinds).toEqual([ 'fireflies', 'teams' ])
    expect(sources.connectors.map((connector) => connector.id)).toEqual([ 'notes', 'chat' ])
  })

  it('offers every providing kind when no source is connected', () => {
    const sources = meetingSourcesOf([ makeDefinition('fireflies', [ 'meetings' ]) ], [ makeConnector({ id: 'jira' }) ])

    expect(sources.kinds).toEqual([ 'fireflies' ])
    expect(sources.connectors).toEqual([])
  })
})
