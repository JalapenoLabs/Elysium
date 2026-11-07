// Copyright © 2026 Jalapeno Labs

// Projects and initiatives are joined and left one at a time through their own routes, so a
// picker's new selection becomes the ids to add and the ids to remove.
export function getMembershipChanges(current: string[], next: string[]) {
  const currentIds = new Set(current)
  const nextIds = new Set(next)
  return {
    added: next.filter((id) => !currentIds.has(id)),
    removed: current.filter((id) => !nextIds.has(id)),
  } as const
}

// The lead once a picker's selection becomes `projectIds`: the lead stays while it is still one of
// them, and otherwise the first of them leads, or none when none is left.
export function getLeadAfter(leadProjectId: string | null, projectIds: string[]) {
  if (leadProjectId && projectIds.includes(leadProjectId)) {
    return leadProjectId
  }
  return projectIds[0] ?? null
}
