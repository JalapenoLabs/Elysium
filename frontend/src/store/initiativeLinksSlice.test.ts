// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  initiativeLinkDeleted,
  initiativeLinksLoaded,
  initiativeLinkUpserted,
  selectInitiativeLinks,
} from './initiativeLinksSlice'

// Misc
import { makeInitiativeLink } from '../testFixtures'

function ids(links: { id: string }[]) {
  return links.map((link) => link.id)
}

describe('initiativeLinksSlice', () => {
  it('replaces one initiative’s containers on load, oldest first, and leaves others alone', () => {
    const store = createAppStore()
    store.dispatch(initiativeLinkUpserted(makeInitiativeLink({ id: 'stale', initiativeId: 'initiative' })))
    store.dispatch(initiativeLinkUpserted(makeInitiativeLink({ id: 'other', initiativeId: 'elsewhere' })))
    store.dispatch(initiativeLinksLoaded({
      initiativeId: 'initiative',
      links: [
        makeInitiativeLink({ id: 'newer', initiativeId: 'initiative', createdAt: '2026-09-02T00:00:00.000Z' }),
        makeInitiativeLink({ id: 'older', initiativeId: 'initiative', createdAt: '2026-09-01T00:00:00.000Z' }),
      ],
    }))

    expect(ids(selectInitiativeLinks(store.getState(), 'initiative'))).toEqual([ 'older', 'newer' ])
    expect(ids(selectInitiativeLinks(store.getState(), 'elsewhere'))).toEqual([ 'other' ])
  })

  it('applies a sync the watcher reports and drops an unlinked container', () => {
    const store = createAppStore()
    store.dispatch(initiativeLinksLoaded({
      initiativeId: 'initiative',
      links: [
        makeInitiativeLink({ id: 'kept', initiativeId: 'initiative' }),
        makeInitiativeLink({ id: 'gone', initiativeId: 'initiative' }),
      ],
    }))
    store.dispatch(initiativeLinkUpserted(makeInitiativeLink({
      id: 'kept',
      initiativeId: 'initiative',
      syncedAt: '2026-09-05T00:00:00.000Z',
    })))
    store.dispatch(initiativeLinkDeleted('gone'))

    const links = selectInitiativeLinks(store.getState(), 'initiative')
    expect(ids(links)).toEqual([ 'kept' ])
    expect(links[0].syncedAt).toBe('2026-09-05T00:00:00.000Z')
  })
})
