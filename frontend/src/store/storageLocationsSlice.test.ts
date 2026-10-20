// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { selectAllStorageLocations, storageLocationsLoaded, storageLocationUpserted } from './storageLocationsSlice'

// Misc
import { makeStorageLocation } from '../testFixtures'

describe('storageLocationUpserted', () => {
  it('moves the Studio default, since at most one location holds it', () => {
    const store = createAppStore()
    store.dispatch(storageLocationsLoaded([
      makeStorageLocation({ id: 'bunny', isStudioDefault: true }),
      makeStorageLocation({ id: 'bucket' }),
    ]))
    store.dispatch(storageLocationUpserted(makeStorageLocation({ id: 'bucket', isStudioDefault: true })))

    const defaults = selectAllStorageLocations(store.getState())
      .filter((location) => location.isStudioDefault)
      .map((location) => location.id)
    expect(defaults).toEqual([ 'bucket' ])
  })
})
