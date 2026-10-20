// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { StudioAsset } from '../api/routes/studioRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Newest first, as the filmstrip reads. The id breaks ties between files kept in the same
// instant, such as a turn's four renders.
const studioAssetsAdapter = createEntityAdapter<StudioAsset>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt)
    || second.id.localeCompare(first.id),
})

// The files of the items viewed. Every version is its own asset, and none is ever changed.
export const studioAssetsSlice = createSlice({
  name: 'studioAssets',
  initialState: studioAssetsAdapter.getInitialState(),
  reducers: {
    // Replaces one item's files, so files removed while the page was away go too.
    studioAssetsLoaded(state, action: PayloadAction<{ studioItemId: string, assets: StudioAsset[] }>) {
      const staleIds: string[] = []
      for (const asset of Object.values(state.entities)) {
        if (asset.studioItemId === action.payload.studioItemId) {
          staleIds.push(asset.id)
        }
      }
      studioAssetsAdapter.removeMany(state, staleIds)
      studioAssetsAdapter.setMany(state, action.payload.assets)
    },
    studioAssetCreated: studioAssetsAdapter.setOne,
  },
})

export const { studioAssetsLoaded, studioAssetCreated } = studioAssetsSlice.actions

const { selectAll: selectAllStudioAssets } = studioAssetsAdapter.getSelectors(
  (state: RootState) => state.studioAssets,
)

// One item's files, newest first.
export const selectStudioItemAssets = createSelector(
  [ selectAllStudioAssets, (_state: RootState, studioItemId: string) => studioItemId ],
  (assets, studioItemId) => assets.filter((asset) => asset.studioItemId === studioItemId),
)
