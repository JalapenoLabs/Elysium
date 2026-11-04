// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  IssueReportCredential,
  IssueReportFiling,
  IssueReportPreview,
  IssueReportReadiness,
} from '../api/routes/issueReportRoutes'

// Why Post to GitHub cannot be pressed yet.
export type ReportBlocker = 'loading' | 'noToken' | 'limitReached' | 'tokenCannotReport'

export const readinessLabels = {
  ready: 'readiness.ready',
  fine_grained: 'readiness.fine_grained',
  missing_scope: 'readiness.missing_scope',
  expired: 'readiness.expired',
} as const satisfies Record<IssueReportReadiness, ParseKeys<'issueReports'>>

export const blockerLabels = {
  loading: 'blocked.loading',
  noToken: 'blocked.noToken',
  limitReached: 'blocked.limitReached',
  tokenCannotReport: 'blocked.tokenCannotReport',
} as const satisfies Record<ReportBlocker, ParseKeys<'issueReports'>>

export const filedToastLabels = {
  issue: 'toasts.opened',
  comment: 'toasts.commented',
} as const satisfies Record<IssueReportFiling, ParseKeys<'issueReports'>>

// Whether a report is sent with a token at all, as `Readiness::may_try` in
// api/src/issue_reports/mod.rs decides: a fine-grained token can only be tried.
export function canTryToken(readiness: IssueReportReadiness) {
  return readiness === 'ready' || readiness === 'fine_grained'
}

// Why the report cannot be posted, or null when it can. `isCurrent` is false while the preview
// on screen is not yet the one for what was typed, since only what was previewed is posted.
export function getReportBlocker(
  preview: IssueReportPreview | undefined,
  credential: IssueReportCredential | null,
  isCurrent: boolean,
): ReportBlocker | null {
  if (!preview || !isCurrent) {
    return 'loading'
  }
  if (!preview.credentials.length) {
    return 'noToken'
  }
  if (preview.remainingToday <= 0) {
    return 'limitReached'
  }
  if (!credential || !canTryToken(credential.readiness)) {
    return 'tokenCannotReport'
  }
  return null
}

// The token a report is filed with: the one picked, while it is still among the person's
// tokens, else the one the API suggests.
export function resolveReportCredential(
  preview: IssueReportPreview | undefined,
  pickedId: string | null,
): IssueReportCredential | null {
  const credentials = preview?.credentials ?? []
  const picked = credentials.find((credential) => credential.id === pickedId)
  if (picked) {
    return picked
  }
  return credentials.find((credential) => credential.id === preview?.suggestedCredentialId) ?? null
}
