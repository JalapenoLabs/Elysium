// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { pageLoaded, pagesLoaded, selectLabelsInUse, selectOpenedPage, selectPageTree } from './pagesSlice'

// Misc
import { makePage, makePageSummary } from '../testFixtures'

describe('pagesSlice', () => {
  it('orders siblings by position, whatever order they arrive in', () => {
    const store = createAppStore()

    store.dispatch(pagesLoaded([
      makePageSummary({ id: 'later', position: 200 }),
      makePageSummary({ id: 'earlier', position: 100 }),
    ]))

    const roots = selectPageTree(store.getState()).childrenByParent.get(null)
    expect(roots?.map((page) => page.id)).toEqual([ 'earlier', 'later' ])
  })

  it('carries a labeled page\'s labels into the tree and the labels in use', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([ makePageSummary({ id: 'page' }) ]))

    store.dispatch(pageLoaded(makePage({ id: 'page', labels: [ 'runbook' ]})))

    expect(selectPageTree(store.getState()).byId.get('page')?.labels).toEqual([ 'runbook' ])
    expect(selectLabelsInUse(store.getState())).toEqual([ 'runbook' ])
  })

  it('names a page opened whole in the tree, and holds its text apart from it', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([ makePageSummary({ id: 'page', title: 'Before' }) ]))

    store.dispatch(pageLoaded(makePage({ id: 'page', title: 'After', body: 'The text' })))

    const summary = selectPageTree(store.getState()).byId.get('page')
    expect(summary?.title).toBe('After')
    expect(summary).not.toHaveProperty('body')
    expect(selectOpenedPage(store.getState(), 'page')?.body).toBe('The text')
  })

  it('takes a deleted page off the tree and keeps it readable', () => {
    const store = createAppStore()
    store.dispatch(pagesLoaded([ makePageSummary({ id: 'page' }) ]))
    const deletedAt = '2026-11-05T12:00:00Z'

    store.dispatch(pageLoaded(makePage({ id: 'page', deletedAt })))

    expect(selectPageTree(store.getState()).byId.has('page')).toBe(false)
    expect(selectOpenedPage(store.getState(), 'page')?.deletedAt).toBe(deletedAt)
  })

  it('keeps pages opened when the list is replaced', () => {
    const store = createAppStore()
    store.dispatch(pageLoaded(makePage({ id: 'page' })))

    store.dispatch(pagesLoaded([]))

    expect(selectPageTree(store.getState()).byId.size).toBe(0)
    expect(selectOpenedPage(store.getState(), 'page')).toBeDefined()
  })
})
