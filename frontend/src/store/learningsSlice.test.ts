// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  deletedLearningsLoaded,
  learningRemoved,
  learningsLoaded,
  learningUpserted,
  selectDeletedLearnings,
  selectLearningById,
  selectLiveLearnings,
} from './learningsSlice'

// Misc
import { makeLearning } from '../testFixtures'

const DELETED_AT = '2026-09-10T00:00:00.000Z'

function ids(learnings: { id: string }[]) {
  return learnings.map((learning) => learning.id)
}

describe('learningsSlice', () => {
  it('keeps live and deleted learnings apart, each load replacing only its own half', () => {
    const store = createAppStore()
    store.dispatch(learningsLoaded([ makeLearning({ id: 'banana' }) ]))
    store.dispatch(deletedLearningsLoaded([ makeLearning({ id: 'apple', deletedAt: DELETED_AT }) ]))
    store.dispatch(learningsLoaded([ makeLearning({ id: 'cherry' }) ]))

    expect(ids(selectLiveLearnings(store.getState()))).toEqual([ 'cherry' ])
    expect(ids(selectDeletedLearnings(store.getState()))).toEqual([ 'apple' ])
  })

  it('lists the most recently changed first, as the API does', () => {
    const store = createAppStore()
    store.dispatch(learningsLoaded([
      makeLearning({ id: 'stale', updatedAt: '2026-09-01T00:00:00.000Z' }),
      makeLearning({ id: 'revised', updatedAt: '2026-09-05T00:00:00.000Z' }),
    ]))

    expect(ids(selectLiveLearnings(store.getState()))).toEqual([ 'revised', 'stale' ])
  })

  it('moves a learning between halves as it is deleted and restored', () => {
    const store = createAppStore()
    store.dispatch(learningsLoaded([ makeLearning({ id: 'banana' }) ]))

    store.dispatch(learningUpserted(makeLearning({ id: 'banana', deletedAt: DELETED_AT })))
    expect(ids(selectLiveLearnings(store.getState()))).toEqual([])
    expect(selectLearningById(store.getState(), 'banana')?.deletedAt).toBe(DELETED_AT)

    store.dispatch(learningUpserted(makeLearning({ id: 'banana' })))
    expect(ids(selectLiveLearnings(store.getState()))).toEqual([ 'banana' ])
    expect(ids(selectDeletedLearnings(store.getState()))).toEqual([])
  })

  it('forgets a learning the API no longer answers for, from either half', () => {
    const store = createAppStore()
    store.dispatch(learningsLoaded([ makeLearning({ id: 'banana' }) ]))
    store.dispatch(deletedLearningsLoaded([ makeLearning({ id: 'apple', deletedAt: DELETED_AT }) ]))

    store.dispatch(learningRemoved('banana'))
    store.dispatch(learningRemoved('apple'))

    expect(selectLearningById(store.getState(), 'banana')).toBeUndefined()
    expect(selectLearningById(store.getState(), 'apple')).toBeUndefined()
  })
})
