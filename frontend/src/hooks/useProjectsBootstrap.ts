// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'

// Redux
import { dispatch } from '../store'
import { useAppSelector } from '../store/hooks'
import {
  activeProjectStorageKey,
  projectsCleared,
  projectsLoaded,
  toProjectView,
} from '../store/projectsSlice'

// Misc
import { listProjects } from '../api/routes/projectRoutes'

// Fetches the active org's projects into the store, restoring the org's last
// active project from localStorage. Reused after mutations (create/rename/
// delete) so every surface (switcher, management page, palette) refreshes from
// one code path. There are no project realtime events yet; when they land this
// becomes the patch target.
export async function reloadProjects(orgId: string) {
  try {
    const response = await listProjects()
    dispatch(projectsLoaded({
      projects: response.projects.map(toProjectView),
      preferredProjectId: localStorage.getItem(activeProjectStorageKey(orgId)),
    }))
  }
  catch (error) {
    console.debug('projects unavailable', { error })
  }
}

// Mounted once in AppLayout: loads on sign-in and reloads on org switch.
export function useProjectsBootstrap() {
  const activeOrgId = useAppSelector((state) => state.sessionContext.activeOrgId)

  useEffect(() => {
    if (!activeOrgId) {
      return
    }
    dispatch(projectsCleared())
    reloadProjects(activeOrgId)
  }, [ activeOrgId ])
}
