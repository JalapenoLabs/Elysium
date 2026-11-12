// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from '../store'
import { meetingsLoaded, meetingUpserted, selectListedMeetings, selectMeetingById } from '../store/meetingsSlice'

// Misc
import { makeMeeting } from '../testFixtures'

// Found building #268: the knowledge base loads the meetings list around every one of its pages, and the
// list leaves out a meeting merged into another. Its answer replaced every meeting in the store, so when it
// landed after a merged-away meeting's own load, that meeting's page said it was not found, notice and all.
describe('a meeting merged away, as its page loads beside the list', () => {
  it('stays for its page when the list answers after it', () => {
    const store = createAppStore()

    // The page loads its meeting by id, then the frame's list answers without it.
    store.dispatch(meetingUpserted(makeMeeting({ id: 'merged', mergedInto: 'kept' })))
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept' }) ]))

    expect(selectMeetingById(store.getState(), 'merged')?.mergedInto).toBe('kept')
    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'kept' ])
  })

  it('takes the list\'s copy once it is split back out, and still lets a list drop what it no longer names', () => {
    const store = createAppStore()
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept' }), makeMeeting({ id: 'gone' }) ]))
    store.dispatch(meetingUpserted(makeMeeting({ id: 'split', mergedInto: 'kept' })))

    // Split meanwhile in another tab, so the store's copy is stale; the list names it as its own.
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept' }), makeMeeting({ id: 'split', mergedInto: null }) ]))

    expect(selectMeetingById(store.getState(), 'split')?.mergedInto).toBeNull()
    expect(selectMeetingById(store.getState(), 'gone')).toBeUndefined()
  })
})
