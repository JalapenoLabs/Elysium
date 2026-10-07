// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { selectStudioItemAssets, studioAssetCreated, studioAssetsLoaded } from './studioAssetsSlice'

// Misc
import { makeStudioAsset } from '../testFixtures'

function ids(assets: { id: string }[]) {
  return assets.map((asset) => asset.id)
}

describe('studioAssetsSlice', () => {
  it('replaces one item\'s files and leaves other items\' alone', () => {
    const store = createAppStore()
    store.dispatch(studioAssetsLoaded({
      studioItemId: 'banana',
      assets: [ makeStudioAsset({ id: 'stale', artifactPath: 'banana.png', studioItemId: 'banana' }) ],
    }))
    store.dispatch(studioAssetsLoaded({
      studioItemId: 'apple',
      assets: [ makeStudioAsset({ id: 'apple-png', artifactPath: 'apple.png', studioItemId: 'apple' }) ],
    }))
    store.dispatch(studioAssetsLoaded({
      studioItemId: 'banana',
      assets: [ makeStudioAsset({ id: 'fresh', artifactPath: 'banana.png', studioItemId: 'banana' }) ],
    }))

    expect(ids(selectStudioItemAssets(store.getState(), 'banana'))).toEqual([ 'fresh' ])
    expect(ids(selectStudioItemAssets(store.getState(), 'apple'))).toEqual([ 'apple-png' ])
  })

  it('lists files newest first as they arrive', () => {
    const store = createAppStore()
    store.dispatch(studioAssetsLoaded({
      studioItemId: 'banana',
      assets: [ makeStudioAsset({ id: 'first', artifactPath: 'banana.png', createdAt: '2026-09-01T00:00:00.000Z' }) ],
    }))
    store.dispatch(studioAssetCreated(
      makeStudioAsset({ id: 'second', artifactPath: 'banana.png', createdAt: '2026-09-02T00:00:00.000Z' }),
    ))

    expect(ids(selectStudioItemAssets(store.getState(), 'item'))).toEqual([ 'second', 'first' ])
  })
})
