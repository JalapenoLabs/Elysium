// Copyright © 2026 Jalapeno Labs

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
  readMeetingFilters,
  toMeetingFilterValues,
  writeMeetingFilters,
} from './meetingFilters'

const NO_FILTERS = readMeetingFilters(new URLSearchParams())

describe('readMeetingFilters and writeMeetingFilters', () => {
  it('reads the plain address as every meeting, and writes nothing for it', () => {
    expect(NO_FILTERS).toEqual({ sources: [], states: [], project: null, waitingForReview: false })
    expect(writeMeetingFilters(NO_FILTERS).toString()).toBe('')
  })

  it('reads back what it writes, leaving out sources and states it does not know', () => {
    const filters: MeetingFilters = {
      sources: [ 'fireflies' ],
      states: [ 'failed', 'done' ],
      project: 'launch',
      waitingForReview: true,
    }
    const params = writeMeetingFilters(filters)
    params.set('source', 'fireflies,somewhere-else')

    expect(readMeetingFilters(params)).toEqual({
      sources: [ 'fireflies' ],
      // In the order the API names states, whatever order the address held.
      states: [ 'done', 'failed' ],
      project: 'launch',
      waitingForReview: true,
    })
  })

  it('reads a project\'s Meetings group link as that project chosen', () => {
    const url = new URL(getProjectMeetingsUrl('launch'), 'http://localhost')

    expect(readMeetingFilters(url.searchParams).project).toBe('launch')
  })

  it('round-trips through the drawer\'s values', () => {
    const filters: MeetingFilters = { sources: [], states: [ 'pending' ], project: NO_PROJECT, waitingForReview: false }

    expect(fromMeetingFilterValues(toMeetingFilterValues(filters))).toEqual(filters)
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
