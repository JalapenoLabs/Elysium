// Copyright © 2026 Jalapeno Labs

// Core
import useSWR from 'swr'

// Misc
import { listProjectFindings, projectFindingsKey } from '../../api/routes/findingRoutes'
import { toLoadStatus } from '../../hooks/useServerData'

// A project's open findings and how many are open, once the company's members ask: a finding
// quotes agents' work, so a guest is never asked for them. Read from SWR alone, since no other
// view lists them, and `finding.changed` revalidates it, so a report or a decision made anywhere
// appears here.
export function useProjectFindings(projectId: string, isEnabled: boolean) {
  const key = isEnabled
    ? projectFindingsKey(projectId)
    : null
  const { data, error } = useSWR(key, () => listProjectFindings(projectId))

  return {
    findings: data?.findings,
    total: data?.total,
    status: toLoadStatus(data !== undefined, error),
  } as const
}
