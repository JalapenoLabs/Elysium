// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Initiative } from '../api/routes/initiativeRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Redux
import { projectDeleted } from './projectsSlice'

// By name, matching the API's list.
const initiativesAdapter = createEntityAdapter<Initiative>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

// Live and deleted initiatives are held apart for the same reason as action items: each
// loads on its own and replaces only its half.
const initialState = {
  live: initiativesAdapter.getInitialState(),
  deleted: initiativesAdapter.getInitialState(),
}

export const initiativesSlice = createSlice({
  name: 'initiatives',
  initialState,
  reducers: {
    initiativesLoaded(state, action: PayloadAction<Initiative[]>) {
      initiativesAdapter.setAll(state.live, action.payload)
    },
    deletedInitiativesLoaded(state, action: PayloadAction<Initiative[]>) {
      initiativesAdapter.setAll(state.deleted, action.payload)
    },
    // Files the initiative under live or deleted by its `deletedAt`.
    initiativeUpserted(state, action: PayloadAction<Initiative>) {
      const initiative = action.payload
      if (initiative.deletedAt) {
        initiativesAdapter.removeOne(state.live, initiative.id)
        initiativesAdapter.setOne(state.deleted, initiative)
        return
      }
      initiativesAdapter.removeOne(state.deleted, initiative.id)
      initiativesAdapter.setOne(state.live, initiative)
    },
    initiativeDeleted(state, action: PayloadAction<string>) {
      initiativesAdapter.removeOne(state.live, action.payload)
    },
  },
  extraReducers: (builder) => {
    // Deleted initiatives are not announced when the API takes them out of a project.
    builder.addCase(projectDeleted, (state, action) => {
      for (const half of [ state.live, state.deleted ]) {
        for (const initiative of Object.values(half.entities)) {
          if (initiative.projectIds.includes(action.payload)) {
            initiative.projectIds = initiative.projectIds.filter((projectId) => projectId !== action.payload)
          }
        }
      }
    })
  },
})

export const {
  initiativesLoaded,
  deletedInitiativesLoaded,
  initiativeUpserted,
  initiativeDeleted,
} = initiativesSlice.actions

export const {
  selectAll: selectLiveInitiatives,
  selectEntities: selectLiveInitiativeEntities,
} = initiativesAdapter.getSelectors((state: RootState) => state.initiatives.live)

export const {
  selectAll: selectDeletedInitiatives,
} = initiativesAdapter.getSelectors((state: RootState) => state.initiatives.deleted)

// Live or deleted, whichever holds it.
export function selectInitiativeById(state: RootState, initiativeId: string) {
  return state.initiatives.live.entities[initiativeId] ?? state.initiatives.deleted.entities[initiativeId]
}

export const selectProjectInitiatives = createSelector(
  [ selectLiveInitiatives, (_state: RootState, projectId: string) => projectId ],
  (initiatives, projectId) => initiatives.filter((initiative) => initiative.projectIds.includes(projectId)),
)

// Names only, for views that label items by initiative. Items never list a deleted
// initiative, so live names are all they need. Pair with shallowEqual.
export const selectInitiativeNamesById = createSelector(
  [ selectLiveInitiativeEntities ],
  (entities) => {
    const namesById: Record<string, string> = {}
    for (const initiative of Object.values(entities)) {
      namesById[initiative.id] = initiative.name
    }
    return namesById
  },
)
