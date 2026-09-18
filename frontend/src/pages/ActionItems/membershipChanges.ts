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
