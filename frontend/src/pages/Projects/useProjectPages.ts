// Copyright © 2026 Jalapeno Labs

// Core
import useSWR from 'swr'

// Misc
import { listProjectPages, projectPagesKey } from '../../api/routes/pageRoutes'
import { toLoadStatus } from '../../hooks/useServerData'

// The pages linked to a project, its docs, once the company's members ask: a guest reaches no knowledge
// base, so is never asked for them. Read from SWR alone, since no other view shows them, and
// `page.changed` revalidates it, so a link made anywhere appears here.
export function useProjectPages(projectId: string, isEnabled: boolean) {
  const key = isEnabled
    ? projectPagesKey(projectId)
    : null
  const { data, error } = useSWR(key, () => listProjectPages(projectId))

  return { pages: data?.pages, status: toLoadStatus(data !== undefined, error) } as const
}
