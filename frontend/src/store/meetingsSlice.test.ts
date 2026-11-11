// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { meetingsLoaded, meetingUpserted, selectListedMeetings, selectMeetingById } from './meetingsSlice'

// Misc
import { makeMeeting } from '../testFixtures'

describe('selectListedMeetings', () => {
  it('leaves out a meeting merged into another, which its own page still reads', () => {
    const store = createAppStore()
    store.dispatch(meetingsLoaded([
      makeMeeting({ id: 'teams', startedAt: '2026-11-10T15:00:00Z' }),
      makeMeeting({ id: 'fireflies', startedAt: '2026-11-10T15:01:00Z' }),
    ]))

    // A merge announces both meetings, and the one merged away arrives saying where it went.
    store.dispatch(meetingUpserted(makeMeeting({
      id: 'fireflies',
      startedAt: '2026-11-10T15:01:00Z',
      mergedInto: 'teams',
    })))

    const listed = selectListedMeetings(store.getState())
    expect(listed.map((meeting) => meeting.id)).toEqual([ 'teams' ])
    expect(selectMeetingById(store.getState(), 'fireflies')?.mergedInto).toBe('teams')
    // Nothing changed, so a list rendering again gets the same list.
    expect(selectListedMeetings(store.getState())).toBe(listed)

    // Split back out, it is listed again.
    store.dispatch(meetingUpserted(makeMeeting({
      id: 'fireflies',
      startedAt: '2026-11-10T15:01:00Z',
      mergedInto: null,
    })))
    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'fireflies', 'teams' ])
  })
})
