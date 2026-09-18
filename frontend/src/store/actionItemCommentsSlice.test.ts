// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  actionItemCommentDeleted,
  actionItemCommentsLoaded,
  actionItemCommentUpserted,
  selectActionItemComments,
} from './actionItemCommentsSlice'

// Misc
import { makeComment } from '../testFixtures'

function bodies(comments: { body: string }[]) {
  return comments.map((comment) => comment.body)
}

describe('actionItemCommentsSlice', () => {
  it('replaces one item’s comments on load and leaves other items’ alone', () => {
    const store = createAppStore()
    store.dispatch(actionItemCommentUpserted(makeComment({ id: 'stale', actionItemId: 'item', body: 'Stale' })))
    store.dispatch(actionItemCommentUpserted(makeComment({ id: 'other', actionItemId: 'elsewhere', body: 'Kept' })))
    store.dispatch(actionItemCommentsLoaded({
      actionItemId: 'item',
      comments: [ makeComment({ id: 'fresh', actionItemId: 'item', body: 'Fresh' }) ],
    }))

    expect(bodies(selectActionItemComments(store.getState(), 'item'))).toEqual([ 'Fresh' ])
    expect(bodies(selectActionItemComments(store.getState(), 'elsewhere'))).toEqual([ 'Kept' ])
  })

  it('reads oldest first and applies edits and deletes', () => {
    const store = createAppStore()
    store.dispatch(actionItemCommentsLoaded({
      actionItemId: 'item',
      comments: [
        makeComment({ id: 'second', actionItemId: 'item', body: 'Second', createdAt: '2026-09-02T00:00:00.000Z' }),
        makeComment({ id: 'first', actionItemId: 'item', body: 'First', createdAt: '2026-09-01T00:00:00.000Z' }),
      ],
    }))
    store.dispatch(actionItemCommentUpserted(makeComment({
      id: 'first',
      actionItemId: 'item',
      body: 'First, edited',
      createdAt: '2026-09-01T00:00:00.000Z',
    })))
    store.dispatch(actionItemCommentDeleted('second'))

    expect(bodies(selectActionItemComments(store.getState(), 'item'))).toEqual([ 'First, edited' ])
  })
})
