// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Project } from '../proto/generated/project_pb'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Redux
import { sessionAnonymous, sessionError } from './sessionSlice'

// The active org's projects and the client-side active project
// (docs/projects.md). The backend has no session-level active project; which
// project's resources a window shows is a viewing preference, chosen here and
// remembered per org in localStorage. Drives, mail, and integrations render in
// the active project's context as those surfaces land.

export type ProjectView = {
  id: string
  name: string
  slug: string
  domains: string[]
}

// Serializable projection of the generated proto message.
export function toProjectView(project: Project): ProjectView {
  return {
    id: project.id?.value ?? '',
    name: project.name,
    slug: project.slug,
    domains: project.domains,
  }
}

// One localStorage key per org, so switching orgs restores each org's last
// active project independently.
export function activeProjectStorageKey(orgId: string): string {
  return `elysium.activeProject.${orgId}`
}

type ProjectsState = {
  projects: ProjectView[]
  activeProjectId: string | null
  isLoaded: boolean
}

const initialState: ProjectsState = {
  projects: [],
  activeProjectId: null,
  isLoaded: false,
}

export const projectsSlice = createSlice({
  name: 'projects',
  initialState,
  reducers: {
    // Replaces the roster after a fetch. The active project resolves in order:
    // the caller's restored preference, the previously active project, then
    // the first project (every org has at least one).
    projectsLoaded(
      state,
      action: PayloadAction<{ projects: ProjectView[], preferredProjectId: string | null }>,
    ) {
      state.projects = action.payload.projects
      state.isLoaded = true

      const candidates = [ action.payload.preferredProjectId, state.activeProjectId ]
      const restored = candidates.find(
        (candidateId) => candidateId
          && action.payload.projects.some((project) => project.id === candidateId),
      )
      state.activeProjectId = restored ?? action.payload.projects[0]?.id ?? null
    },
    activeProjectChanged(state, action: PayloadAction<string>) {
      if (!state.projects.some((project) => project.id === action.payload)) {
        console.debug('ignoring switch to a project not in the roster', { projectId: action.payload })
        return
      }
      state.activeProjectId = action.payload
    },
    // Cleared while an org switch refetches, so no stale roster renders.
    projectsCleared() {
      return initialState
    },
  },
  extraReducers(builder) {
    builder.addCase(sessionAnonymous, () => initialState)
    builder.addCase(sessionError, () => initialState)
  },
})

export const {
  projectsLoaded,
  activeProjectChanged,
  projectsCleared,
} = projectsSlice.actions
export const projectsReducer = projectsSlice.reducer
