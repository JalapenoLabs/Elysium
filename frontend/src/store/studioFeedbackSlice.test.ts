// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { selectStudioItemFeedback, studioFeedbackCreated, studioFeedbackLoaded } from './studioFeedbackSlice'

// Misc
import { makeStudioFeedback } from '../testFixtures'

function ids(feedback: { id: string }[]) {
  return feedback.map((entry) => entry.id)
}

describe('studioFeedbackSlice', () => {
  it('replaces one item\'s feedback and lists it oldest first', () => {
    const store = createAppStore()
    store.dispatch(studioFeedbackLoaded({
      studioItemId: 'item',
      feedback: [ makeStudioFeedback({ id: 'stale' }) ],
    }))
    store.dispatch(studioFeedbackLoaded({
      studioItemId: 'item',
      feedback: [
        makeStudioFeedback({ id: 'later', createdAt: '2026-09-03T00:00:00.000Z' }),
        makeStudioFeedback({ id: 'earlier', createdAt: '2026-09-02T00:00:00.000Z' }),
      ],
    }))

    expect(ids(selectStudioItemFeedback(store.getState(), 'item'))).toEqual([ 'earlier', 'later' ])
  })

  it('adds a drawn prompt as it is sent', () => {
    const store = createAppStore()
    store.dispatch(studioFeedbackLoaded({ studioItemId: 'item', feedback: [] }))
    store.dispatch(studioFeedbackCreated(makeStudioFeedback({ id: 'new', turnId: 'turn-1' })))

    expect(selectStudioItemFeedback(store.getState(), 'item')[0]?.turnId).toBe('turn-1')
    expect(selectStudioItemFeedback(store.getState(), 'other')).toEqual([])
  })
})
