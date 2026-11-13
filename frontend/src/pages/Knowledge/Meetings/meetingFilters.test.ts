// Copyright © 2026 Jalapeno Labs

import type { ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'
import type { MeetingFilters } from './meetingFilters'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeMeeting } from '../../../testFixtures'
import { getProjectMeetingsUrl } from '../../../urls'
import { NO_PROJECT } from '../../ActionItems/actionItemFilters'
import {
  filterMeetings,
  fromMeetingFilterValues,
  listMeetingSourceKinds,
  readMeetingFilters,
  toMeetingFilterValues,
  writeMeetingFilters,
} from './meetingFilters'

const NO_FILTERS = readMeetingFilters(new URLSearchParams())

describe('readMeetingFilters and writeMeetingFilters', () => {
  it('reads the plain address as every meeting, and writes nothing for it', () => {
    expect(NO_FILTERS).toEqual({ sources: [], states: [], project: null, waitingForReview: false, deleted: false })
    expect(writeMeetingFilters(NO_FILTERS).toString()).toBe('')
  })

  it('keeps the deleted meetings\' view in the address, so it survives opening one and coming back', () => {
    const params = writeMeetingFilters({ ...NO_FILTERS, deleted: true })

    expect(params.toString()).toBe('deleted=true')
    expect(readMeetingFilters(params).deleted).toBe(true)
    // The drawer shows it as a tag, and live meetings, the plain list, as none.
    expect(toMeetingFilterValues({ ...NO_FILTERS, deleted: true }).deleted).toBe(true)
    expect(toMeetingFilterValues(NO_FILTERS).deleted).toBeNull()
  })

  it('reads back what it writes, keeping any source and leaving out states it does not know', () => {
    const filters: MeetingFilters = {
      sources: [ 'fireflies', 'upload' ],
      states: [ 'failed', 'done' ],
      project: 'launch',
      waitingForReview: true,
      deleted: false,
    }
    const params = writeMeetingFilters(filters)
    params.set('state', 'failed,done,somewhere-else')

    expect(readMeetingFilters(params)).toEqual({
      // A source need not be a connector, as an upload is not.
      sources: [ 'fireflies', 'upload' ],
      // In the order the API names states, whatever order the address held.
      states: [ 'done', 'failed' ],
      project: 'launch',
      waitingForReview: true,
      deleted: false,
    })
  })

  it('reads a project\'s Meetings group link as that project chosen', () => {
    const url = new URL(getProjectMeetingsUrl('launch'), 'http://localhost')

    expect(readMeetingFilters(url.searchParams).project).toBe('launch')
  })

  it('round-trips through the drawer\'s values', () => {
    const filters: MeetingFilters = {
      sources: [],
      states: [ 'pending' ],
      project: NO_PROJECT,
      waitingForReview: false,
      deleted: true,
    }

    expect(fromMeetingFilterValues(toMeetingFilterValues(filters))).toEqual(filters)
  })
})

describe('listMeetingSourceKinds', () => {
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

  it('offers every kind that provides meetings and uploads, read or not, then any other source a meeting names', () => {
    const definitions = [
      makeDefinition('jira', [ 'issues' ]),
      makeDefinition('teams', [ 'messages', 'meetings' ]),
      makeDefinition('slack', [ 'messages', 'meetings' ]),
      makeDefinition('fireflies', [ 'meetings' ]),
      makeDefinition('fireflies_browser', [ 'meetings' ]),
    ]
    // A source a later build adds, which this one has no connector kind for.
    const meetings = [
      makeMeeting({ id: 'recorded', sourceKind: 'zoom' }),
      makeMeeting({ id: 'standup', sourceKind: 'fireflies' }),
      makeMeeting({ id: 'uploaded', sourceKind: 'upload' }),
    ]

    expect(listMeetingSourceKinds(definitions, meetings))
      .toEqual([ 'teams', 'slack', 'fireflies', 'fireflies_browser', 'upload', 'zoom' ])
  })
})

describe('filterMeetings', () => {
  const meetings = [
    makeMeeting({ id: 'standup', projectIds: [ 'launch' ], awaitingApproval: true, state: 'recorded' }),
    makeMeeting({ id: 'one-to-one', projectIds: []}),
    makeMeeting({ id: 'retro', projectIds: [ 'billing' ], state: 'failed' }),
  ]

  it('keeps everything with no filter, and counts what waits for review', () => {
    const { matches, waitingCount } = filterMeetings(meetings, NO_FILTERS)

    expect(matches.map((meeting) => meeting.id)).toEqual([ 'standup', 'one-to-one', 'retro' ])
    expect(waitingCount).toBe(1)
  })

  it('narrows to a project, or to the meetings on none', () => {
    const onLaunch = filterMeetings(meetings, { ...NO_FILTERS, project: 'launch' })
    const onNone = filterMeetings(meetings, { ...NO_FILTERS, project: NO_PROJECT })

    expect(onLaunch.matches.map((meeting) => meeting.id)).toEqual([ 'standup' ])
    expect(onNone.matches.map((meeting) => meeting.id)).toEqual([ 'one-to-one' ])
  })

  it('narrows by state and by review, counting the waiting ones whatever is left', () => {
    const failed = filterMeetings(meetings, { ...NO_FILTERS, states: [ 'failed' ]})
    const waiting = filterMeetings(meetings, { ...NO_FILTERS, waitingForReview: true })

    expect(failed.matches.map((meeting) => meeting.id)).toEqual([ 'retro' ])
    expect(failed.waitingCount).toBe(1)
    expect(waiting.matches.map((meeting) => meeting.id)).toEqual([ 'standup' ])
  })

  it('narrows by source', () => {
    const { matches } = filterMeetings(meetings, { ...NO_FILTERS, sources: [ 'youtrack' ]})

    expect(matches).toEqual([])
  })
})
