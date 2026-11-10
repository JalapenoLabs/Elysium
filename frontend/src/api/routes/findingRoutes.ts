// Copyright © 2026 Jalapeno Labs

import type { LinkProvider } from './actionItemRoutes'
import type { WorkBatchState } from './projectManagerRoutes'

// Misc
import { apiClient } from '../index'

// What a finding would cost to act on, as the agent that reported it estimates it. Mirrors `Cost`
// in api/src/models/finding.rs.
export type FindingCost = 'small' | 'medium' | 'large'

// Where a finding stands. Mirrors `FindingState` in api/src/models/finding.rs.
export type FindingState = 'open' | 'accepted' | 'dismissed'

// Work a worker or a Project Manager noticed that nobody asked for. Mirrors `FindingResponse` in
// api/src/routes/v1/findings/mod.rs.
export type Finding = {
  id: string
  projectId: string
  title: string
  // What was seen, why it matters, and what acting on it involves.
  detail: string
  // The repository it was seen in, when it was seen in one.
  repositoryUrl: string | null
  // Where in it, or where else: a path, a URL, or a place's name.
  location: string
  cost: FindingCost
  state: FindingState
  // The batch whose worker reported it first, or the Project Manager's turn that did.
  workBatchId: string | null
  projectManagerTurnId: string | null
  // How many times it was reported, the first included.
  reports: number
  lastReportedAt: string
  decidedBy: string | null
  decidedAt: string | null
  dismissalReason: string | null
  // What accepting it made.
  actionItemId: string | null
  acceptedBatchId: string | null
  createdBy: string
  createdAt: string
}

// The most characters a dismissal's reason holds, matching `findings.dismissal_reason`.
export const DISMISSAL_REASON_MAX_CHARACTERS = 2_000

// The SWR key `finding.changed` revalidates while a view holds it.
export function projectFindingsKey(projectId: string) {
  return `v1/projects/${projectId}/findings`
}

type ListProjectFindingsResponse = {
  // Open findings, newest first, at most 200.
  findings: Finding[]
  // How many are open in all, however many are listed.
  total: number
}

// A project's open findings and their count; members alone.
export function listProjectFindings(projectId: string) {
  return apiClient
    .get(projectFindingsKey(projectId))
    .json<ListProjectFindingsResponse>()
}

type AcceptFindingResponse = {
  finding: Finding
  itemId: string
  batchId: string
  // `proposed`, waiting for the person's approval, or `queued` when the project's level of
  // autonomy runs work a person asked for at once.
  batchState: WorkBatchState
  // Where its issue files, or null when the project has nowhere to file it.
  issue: { provider: LinkProvider, destination: string } | null
  // Why it files nowhere, in the API's words, when `issue` is null.
  issueRefusal: string | null
}

// Accepts an open finding: it becomes an action item whose issue files in the project's
// tracker, and a batch for it, proposed or queued as the project's level of autonomy says.
// `409 finding_decided` once someone decided it.
export function acceptFinding(findingId: string) {
  return apiClient
    .post(`v1/findings/${findingId}/accept`)
    .json<AcceptFindingResponse>()
}

type DismissFindingResponse = {
  finding: Finding
}

// Dismisses an open finding for good, saying why when a reason is given; the agents that see it
// again are told. `409 finding_decided` once someone decided it.
export function dismissFinding(findingId: string, reason: string | null) {
  return apiClient
    .post(`v1/findings/${findingId}/dismiss`, { json: { reason }})
    .json<DismissFindingResponse>()
}
