// Copyright © 2026 Jalapeno Labs

import type { Project } from '../../api/routes/projectRoutes'

// How the Projects page lists projects. Both views show the same search results in
// the same order, so the filtering and sorting live here rather than in either view.

export const PROJECT_VIEWS = [ 'table', 'tiles' ] as const
export type ProjectView = typeof PROJECT_VIEWS[number]

export const PROJECT_SORT_KEYS = [ 'name', 'sessions', 'updated' ] as const
export type ProjectSortKey = typeof PROJECT_SORT_KEYS[number]

export type ProjectSort = {
  key: ProjectSortKey
  descending: boolean
}

export const DEFAULT_PROJECT_SORT: ProjectSort = {
  key: 'name',
  descending: false,
}

type Comparator = (first: Project, second: Project, sessionCounts: Record<string, number>) => number

// Ascending order per key. Ties fall back to the name so the order never shuffles.
const compareBySortKey = {
  name: (first, second) => first.name.localeCompare(second.name),
  sessions: (first, second, sessionCounts) => (sessionCounts[first.id] ?? 0) - (sessionCounts[second.id] ?? 0),
  updated: (first, second) => first.updatedAt.localeCompare(second.updatedAt),
} as const satisfies Record<ProjectSortKey, Comparator>

// Projects whose name or description contains `search`, case-insensitively, in `sort` order.
export function searchAndSortProjects(
  projects: Project[],
  sessionCounts: Record<string, number>,
  search: string,
  sort: ProjectSort,
): Project[] {
  const needle = search.trim().toLocaleLowerCase()
  const matches = needle
    ? projects.filter((project) => `${project.name}\n${project.description}`.toLocaleLowerCase().includes(needle))
    : [ ...projects ]

  const compare = compareBySortKey[sort.key]
  const direction = sort.descending
    ? -1
    : 1

  return matches.sort((first, second) => {
    const order = compare(first, second, sessionCounts) * direction
    if (order !== 0) {
      return order
    }
    return first.name.localeCompare(second.name)
  })
}
