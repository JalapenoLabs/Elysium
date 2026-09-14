// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Satellite, SatelliteStatus } from '../api/routes/satelliteRoutes'
import type { LoadStatus } from './loadStatus'
import type { RootState } from './index'

// Core
import { createAsyncThunk, createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Misc
import { listSatellites } from '../api/routes/satelliteRoutes'

const satellitesAdapter = createEntityAdapter<Satellite>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

export const fetchSatellites = createAsyncThunk('satellites/fetch', async () => {
  const response = await listSatellites()
  return response.satellites
})

export const satellitesSlice = createSlice({
  name: 'satellites',
  initialState: satellitesAdapter.getInitialState({ status: 'idle' as LoadStatus }),
  reducers: {
    // A saved satellite arrives without a status: the API restarts its watcher, and
    // `satelliteStatusReported` follows once the first poll lands.
    satelliteUpserted: satellitesAdapter.upsertOne,
    satelliteDeleted(state, action: PayloadAction<string>) {
      satellitesAdapter.removeOne(state, action.payload)
    },
    satelliteStatusReported(state, action: PayloadAction<SatelliteStatus>) {
      const satellite = state.entities[action.payload.satelliteId]
      if (!satellite) {
        console.debug('Status reported for an unknown satellite', { status: action.payload })
        return
      }
      satellite.status = action.payload
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchSatellites.pending, (state) => {
        if (state.status !== 'loaded') {
          state.status = 'loading'
        }
      })
      .addCase(fetchSatellites.fulfilled, (state, action) => {
        satellitesAdapter.setAll(state, action.payload)
        state.status = 'loaded'
      })
      .addCase(fetchSatellites.rejected, (state, action) => {
        console.debug('Loading satellites failed', { error: action.error })
        if (state.status !== 'loaded') {
          state.status = 'failed'
        }
      })
  },
})

export const { satelliteUpserted, satelliteDeleted, satelliteStatusReported } = satellitesSlice.actions

export const {
  selectAll: selectAllSatellites,
  selectById: selectSatelliteById,
  selectEntities: selectSatelliteEntities,
} = satellitesAdapter.getSelectors((state: RootState) => state.satellites)

// Names only, for views that label rows by satellite. Status reports replace the
// entities on every poll that finds a change; pair this with shallowEqual so those
// views do not rebuild when no name changed.
export const selectSatelliteNamesById = createSelector(
  [ selectSatelliteEntities ],
  (entities) => {
    const namesById: Record<string, string> = {}
    for (const satellite of Object.values(entities)) {
      namesById[satellite.id] = satellite.name
    }
    return namesById
  },
)

export function selectSatellitesStatus(state: RootState) {
  return state.satellites.status
}
