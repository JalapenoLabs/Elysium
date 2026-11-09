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

// The lead a person has chosen once a picker's selection becomes `projectIds`: it stays while it is
// still one of them, and otherwise none is chosen. Saving then lets the server pass the lead on (the
// surest project left, a person's choice before a guess), because the form sets a lead only when the
// person picked one, and a lead set that way is a person's choice that Elysia never moves.
export function getLeadAfter(leadProjectId: string | null, projectIds: string[]) {
  if (leadProjectId && projectIds.includes(leadProjectId)) {
    return leadProjectId
  }
  return null
}
