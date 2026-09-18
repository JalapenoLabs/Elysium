// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { Project } from '../../api/routes/projectRoutes'

// The projects a session may belong to, mirroring the API: a session started from an action
// item belongs to one of the item's projects, or to any project when the item has none.
export function getSessionProjectChoices(actionItem: ActionItem | null, projects: Project[]) {
  if (!actionItem?.projectIds.length) {
    return projects
  }
  const itemProjectIds = new Set(actionItem.projectIds)
  return projects.filter((project) => itemProjectIds.has(project.id))
}
