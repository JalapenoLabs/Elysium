// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { StorageLocation } from '../api/routes/storageRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Redux
import { projectDeleted } from './projectsSlice'

const storageLocationsAdapter = createEntityAdapter<StorageLocation>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

export const storageLocationsSlice = createSlice({
  name: 'storageLocations',
  initialState: storageLocationsAdapter.getInitialState(),
  reducers: {
    storageLocationsLoaded: storageLocationsAdapter.setAll,
    storageLocationUpserted: storageLocationsAdapter.upsertOne,
    storageLocationDeleted(state, action: PayloadAction<string>) {
      storageLocationsAdapter.removeOne(state, action.payload)
    },
  },
  extraReducers: (builder) => {
    // The API drops a deleted project's links; mirror that rather than refetch.
    builder.addCase(projectDeleted, (state, action) => {
      for (const location of Object.values(state.entities)) {
        if (Array.isArray(location.projects)) {
          location.projects = location.projects.filter((projectId) => projectId !== action.payload)
        }
      }
    })
  },
})

export const {
  storageLocationsLoaded,
  storageLocationUpserted,
  storageLocationDeleted,
} = storageLocationsSlice.actions

export const {
  selectAll: selectAllStorageLocations,
  selectById: selectStorageLocationById,
} = storageLocationsAdapter.getSelectors((state: RootState) => state.storageLocations)
