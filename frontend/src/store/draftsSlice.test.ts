// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { draftRemoved, draftUpserted, recordDraftsLoaded, selectUnsentRecordDrafts } from './draftsSlice'

// Misc
import { makeDraft } from '../testFixtures'

describe('draftsSlice', () => {
  it('lists a record\'s unsent drafts newest first, leaving out what was sent', () => {
    const store = createAppStore()
    store.dispatch(recordDraftsLoaded({
      recordId: 'record-1',
      drafts: [
        makeDraft({ id: 'older', replyToRecordId: 'record-1', createdAt: '2026-11-01T00:00:00Z' }),
        makeDraft({ id: 'newer', replyToRecordId: 'record-1', createdAt: '2026-11-02T00:00:00Z' }),
        makeDraft({ id: 'elsewhere', replyToRecordId: 'record-2' }),
      ],
    }))
    store.dispatch(draftUpserted(makeDraft({
      id: 'older',
      replyToRecordId: 'record-1',
      createdAt: '2026-11-01T00:00:00Z',
      state: 'sent',
    })))

    const unsent = selectUnsentRecordDrafts(store.getState(), 'record-1')
    expect(unsent.map((draft) => draft.id)).toEqual([ 'newer' ])
  })

  it('a record\'s load replaces what was held for it, so a draft discarded elsewhere goes', () => {
    const store = createAppStore()
    store.dispatch(draftUpserted(makeDraft({ id: 'discarded', replyToRecordId: 'record-1' })))
    store.dispatch(draftUpserted(makeDraft({ id: 'other-record', replyToRecordId: 'record-2' })))
    store.dispatch(recordDraftsLoaded({ recordId: 'record-1', drafts: []}))

    expect(selectUnsentRecordDrafts(store.getState(), 'record-1')).toEqual([])
    expect(selectUnsentRecordDrafts(store.getState(), 'record-2')).toHaveLength(1)
  })

  it('a removed draft is gone', () => {
    const store = createAppStore()
    store.dispatch(draftUpserted(makeDraft({ id: 'draft-1', replyToRecordId: 'record-1' })))
    store.dispatch(draftRemoved('draft-1'))
    expect(selectUnsentRecordDrafts(store.getState(), 'record-1')).toEqual([])
  })
})
