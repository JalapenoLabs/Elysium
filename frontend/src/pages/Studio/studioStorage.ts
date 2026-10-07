// Copyright © 2026 Jalapeno Labs

import type { StorageLocation } from '../../api/routes/storageRoutes'

// Misc
import { ALL_PROJECTS } from '../../api/routes/projectRoutes'

// The locations a new item may keep its files in, mirroring the API's rule: with a project,
// those for that project or for every project; without one, only those for every project.
export function getStudioLocationChoices(locations: StorageLocation[], projectId: string | null) {
  return locations.filter((location) => {
    if (location.projects === ALL_PROJECTS) {
      return true
    }
    return projectId !== null && location.projects.includes(projectId)
  })
}

// What the location field holds before the user chooses: Studio's default when it is among
// the choices, otherwise a lone choice, otherwise nothing.
export function getDefaultStudioLocationId(choices: StorageLocation[]) {
  const studioDefault = choices.find((location) => location.isStudioDefault)
  if (studioDefault) {
    return studioDefault.id
  }
  if (choices.length === 1) {
    return choices[0].id
  }
  return ''
}
