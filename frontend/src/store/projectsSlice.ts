// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Project } from '../api/routes/projectRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

const projectsAdapter = createEntityAdapter<Project>({
  sortComparer: (first, second) => first.name.localeCompare(second.name),
})

export const projectsSlice = createSlice({
  name: 'projects',
  initialState: projectsAdapter.getInitialState(),
  reducers: {
    projectsLoaded: projectsAdapter.setAll,
    projectUpserted: projectsAdapter.upsertOne,
    projectDeleted(state, action: PayloadAction<string>) {
      projectsAdapter.removeOne(state, action.payload)
    },
  },
})

export const { projectsLoaded, projectUpserted, projectDeleted } = projectsSlice.actions

export const {
  selectAll: selectAllProjects,
  selectEntities: selectProjectEntities,
} = projectsAdapter.getSelectors((state: RootState) => state.projects)

// Names only, for views that label rows by project. Pair with shallowEqual so those
// views do not rebuild when no name changed.
export const selectProjectNamesById = createSelector(
  [ selectProjectEntities ],
  (entities) => {
    const namesById: Record<string, string> = {}
    for (const project of Object.values(entities)) {
      namesById[project.id] = project.name
    }
    return namesById
  },
)
