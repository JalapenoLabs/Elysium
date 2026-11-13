// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  deletedMeetingsLoaded,
  meetingRemoved,
  meetingsLoaded,
  meetingsUpserted,
  meetingUpserted,
  selectDeletedMeetings,
  selectListedMeetings,
  selectMeetingById,
} from './meetingsSlice'
import { pagesLoaded, selectKnowledgeTree } from './pagesSlice'

// Misc
import { MEETINGS_PAGE_ID } from '../constants'
import { makeMeeting, makePageSummary } from '../testFixtures'

describe('meetingsSlice', () => {
  it('orders meetings newest first, the later id first between two that started together', () => {
    const store = createAppStore()

    store.dispatch(meetingsLoaded([
      makeMeeting({ id: 'a', startedAt: '2026-11-09T09:00:00Z' }),
      makeMeeting({ id: 'b', startedAt: '2026-11-10T09:00:00Z' }),
      makeMeeting({ id: 'c', startedAt: '2026-11-09T09:00:00Z' }),
    ]))

    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'b', 'c', 'a' ])
  })

  it('keeps a meeting loaded after it was merged away out of every list, but not out of the store', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([ makePageSummary({ id: MEETINGS_PAGE_ID, title: 'Meetings', fixed: true }) ]))
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept' }) ]))

    // An event about one of its old topics loads the merged meeting by id.
    store.dispatch(meetingUpserted(makeMeeting({ id: 'merged', mergedInto: 'kept' })))

    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'kept' ])
    const homes = selectKnowledgeTree(store.getState()).meetingsByPage.get(MEETINGS_PAGE_ID)
    expect(homes?.entries.map((entry) => entry.meetingId)).toEqual([ 'kept' ])
    expect(selectMeetingById(store.getState(), 'merged')?.mergedInto).toBe('kept')
  })

  it('drops a meeting its reader may no longer read from every list and the tree', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([ makePageSummary({ id: MEETINGS_PAGE_ID, title: 'Meetings', fixed: true }) ]))
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'open' }), makeMeeting({ id: 'made-private' }) ]))

    // Made private, it answers this reader 404, and leaves.
    store.dispatch(meetingRemoved('made-private'))

    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'open' ])
    const homes = selectKnowledgeTree(store.getState()).meetingsByPage.get(MEETINGS_PAGE_ID)
    expect(homes?.entries.map((entry) => entry.meetingId)).toEqual([ 'open' ])
    expect(selectMeetingById(store.getState(), 'made-private')).toBeUndefined()
  })

  it('lists a meeting again once its captures are split back out', () => {
    const store = createAppStore()
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept', startedAt: '2026-11-10T09:00:00Z' }) ]))
    store.dispatch(meetingUpserted(makeMeeting({ id: 'merged', mergedInto: 'kept' })))

    // The split announces both meetings, and the one split out arrives its own again.
    store.dispatch(meetingUpserted(makeMeeting({ id: 'merged', startedAt: '2026-11-09T09:00:00Z', mergedInto: null })))

    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'kept', 'merged' ])
  })

  // #258: a deleted meeting leaves every list and the tree, and the list that loads around every
  // page keeps it for its own page and the Deleted filter until it is restored.
  it('keeps a deleted meeting out of every list but the deleted one, through the next listing', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([ makePageSummary({ id: MEETINGS_PAGE_ID, title: 'Meetings', fixed: true }) ]))
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept' }), makeMeeting({ id: 'gone' }) ]))

    store.dispatch(meetingsUpserted([ makeMeeting({ id: 'gone', deletedAt: '2026-11-11T09:00:00Z' }) ]))
    // The list loaded around the next page no longer names it.
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept' }) ]))

    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'kept' ])
    expect(selectDeletedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'gone' ])
    const homes = selectKnowledgeTree(store.getState()).meetingsByPage.get(MEETINGS_PAGE_ID)
    expect(homes?.entries.map((entry) => entry.meetingId)).toEqual([ 'kept' ])
    expect(selectMeetingById(store.getState(), 'gone')?.deletedAt).toBe('2026-11-11T09:00:00Z')

    // Restored, it is listed again.
    store.dispatch(meetingUpserted(makeMeeting({ id: 'gone', deletedAt: null })))
    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id).sort()).toEqual([ 'gone', 'kept' ])
    expect(selectDeletedMeetings(store.getState())).toEqual([])
  })

  it('replaces the deleted meetings with what the deleted list answers, keeping the rest', () => {
    const store = createAppStore()
    store.dispatch(meetingsLoaded([ makeMeeting({ id: 'kept' }) ]))
    store.dispatch(meetingUpserted(makeMeeting({ id: 'restored elsewhere', deletedAt: '2026-11-10T09:00:00Z' })))

    store.dispatch(deletedMeetingsLoaded([ makeMeeting({ id: 'gone', deletedAt: '2026-11-11T09:00:00Z' }) ]))

    expect(selectDeletedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'gone' ])
    expect(selectListedMeetings(store.getState()).map((meeting) => meeting.id)).toEqual([ 'kept' ])
  })
})
