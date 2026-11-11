// Copyright © 2026 Jalapeno Labs

import type { ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { MEETING_STATES } from '../../../api/routes/meetingRoutes'
import { makeConnector, makeMeeting } from '../../../testFixtures'
import {
  correctionStatusChipColors,
  correctionStatusLabelKeys,
  correctionStatusOf,
  meetingSourcesOf,
  meetingStateChipColors,
  meetingStateLabelKeys,
} from './meetingsPresentation'

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
  return {
    kind,
    provides,
    scope: 'personal',
    account: 'browser',
    startUrl: null,
    content: [],
    checks: [],
    replies: { through: 'never' },
  }
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

describe('correctionStatusOf', () => {
  const correction = {
    changedLines: 2,
    approvedAt: null,
    approvedBy: null,
    reviewItemId: null,
    createdAt: '2026-11-04T12:00:00Z',
  }

  it('says a meeting is waiting on its person only while its run is held', () => {
    expect(correctionStatusOf(makeMeeting({ id: 'm' }))).toBe('uncorrected')
    expect(correctionStatusOf(makeMeeting({ id: 'm', correction, awaitingApproval: true })))
      .toBe('awaitingApproval')
    // Resumed from the Runs page, the run went on without an approval.
    expect(correctionStatusOf(makeMeeting({ id: 'm', correction }))).toBe('unapproved')
    expect(correctionStatusOf(makeMeeting({
      id: 'm',
      correction: { ...correction, approvedAt: '2026-11-04T13:00:00Z', approvedBy: 'u' },
    }))).toBe('approved')
  })

  it('has a label and a color for every status', () => {
    for (const status of [ 'uncorrected', 'awaitingApproval', 'approved', 'unapproved' ] as const) {
      expect(correctionStatusLabelKeys[status]).toBe(`correction.status.${status}`)
      expect(correctionStatusChipColors[status]).toBeTruthy()
    }
  })
})

