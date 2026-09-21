// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  actionItemLinkDeleted,
  actionItemLinksLoaded,
  actionItemLinkUpserted,
  selectActionItemLinks,
} from './actionItemLinksSlice'

// Misc
import { makeActionItemLink } from '../testFixtures'

function ids(links: { id: string }[]) {
  return links.map((link) => link.id)
}

describe('actionItemLinksSlice', () => {
  it('replaces one item’s links on load and leaves other items’ alone', () => {
    const store = createAppStore()
    store.dispatch(actionItemLinkUpserted(makeActionItemLink({ id: 'stale', actionItemId: 'item' })))
    store.dispatch(actionItemLinkUpserted(makeActionItemLink({ id: 'other', actionItemId: 'elsewhere' })))
    store.dispatch(actionItemLinksLoaded({
      actionItemId: 'item',
      links: [ makeActionItemLink({ id: 'fresh', actionItemId: 'item' }) ],
    }))

    expect(ids(selectActionItemLinks(store.getState(), 'item'))).toEqual([ 'fresh' ])
    expect(ids(selectActionItemLinks(store.getState(), 'elsewhere'))).toEqual([ 'other' ])
  })

  it('lists the primary first, then oldest first', () => {
    const store = createAppStore()
    store.dispatch(actionItemLinksLoaded({
      actionItemId: 'item',
      links: [
        makeActionItemLink({ id: 'newer', actionItemId: 'item', createdAt: '2026-09-03T00:00:00.000Z' }),
        makeActionItemLink({ id: 'older', actionItemId: 'item', createdAt: '2026-09-01T00:00:00.000Z' }),
        makeActionItemLink({
          id: 'primary',
          actionItemId: 'item',
          isPrimary: true,
          createdAt: '2026-09-02T00:00:00.000Z',
        }),
      ],
    }))

    expect(ids(selectActionItemLinks(store.getState(), 'item'))).toEqual([ 'primary', 'older', 'newer' ])
  })

  it('moves a link that becomes primary to the front, and drops a deleted one', () => {
    const store = createAppStore()
    store.dispatch(actionItemLinksLoaded({
      actionItemId: 'item',
      links: [
        makeActionItemLink({ id: 'first', actionItemId: 'item', isPrimary: true }),
        makeActionItemLink({ id: 'second', actionItemId: 'item', createdAt: '2026-09-02T00:00:00.000Z' }),
      ],
    }))
    store.dispatch(actionItemLinkDeleted('first'))
    store.dispatch(actionItemLinkUpserted(makeActionItemLink({
      id: 'second',
      actionItemId: 'item',
      isPrimary: true,
      createdAt: '2026-09-02T00:00:00.000Z',
    })))

    const links = selectActionItemLinks(store.getState(), 'item')
    expect(ids(links)).toEqual([ 'second' ])
    expect(links[0].isPrimary).toBe(true)
  })
})
