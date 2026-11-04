// Copyright © 2026 Jalapeno Labs

import type { GithubTokenKind } from './githubRoutes'

// Misc
import { ISSUE_REPORT_FILE_TIMEOUT_MS } from '../../constants'
import { apiClient } from '../index'

// What a person reports, mirroring `Subject` in api/src/issue_reports/subject.rs. A subject that
// names something Elysium holds is read from its records on the server; `error` carries what any
// other surface showed, named by where it was shown.
export type IssueReportSubject =
  | { kind: 'connectorSync', connectorId: string, runId?: string }
  | { kind: 'connectorCheck', connectorId: string, checkId: string }
  | { kind: 'checkRun', runId: string, checkId: string }
  | { kind: 'error', area: string, message: string }

// Mirrors `Readiness` in api/src/issue_reports/mod.rs: whether a token can file a report, as far
// as Elysium can tell before trying.
export type IssueReportReadiness = 'ready' | 'fine_grained' | 'missing_scope' | 'expired'

export type IssueReportCredential = {
  id: string
  name: string
  kind: GithubTokenKind
  // The account the report is filed as.
  login: string
  isDefault: boolean
  readiness: IssueReportReadiness
}

export type IssueReportFiling = 'issue' | 'comment'

export type IssueReportPreview = {
  // Exactly what is posted: the title, if it opens an issue, and the body either way.
  draft: {
    title: string
    body: string
    fingerprint: string
  }
  // `owner/name` of the repository reports are filed on.
  repository: string
  // The reporter's own GitHub tokens.
  credentials: IssueReportCredential[]
  suggestedCredentialId: string | null
  // The last time this person reported this problem, if they did.
  previousReport: {
    issueNumber: number
    issueUrl: string
    url: string
    filedAs: IssueReportFiling
    reportedAt: string
  } | null
  remainingToday: number
}

export type FiledIssueReport = {
  filedAs: IssueReportFiling
  issueNumber: number
  issueUrl: string
  // Where this report reads: the issue it opened, or its comment.
  url: string
}

type IssueReportRequest = {
  subject: IssueReportSubject
  note?: string
}

export function previewIssueReport(body: IssueReportRequest) {
  return apiClient
    .post('v1/issue-reports/preview', { json: body })
    .json<IssueReportPreview>()
}

type FileIssueReportRequest = IssueReportRequest & {
  credentialId: string
}

type FileIssueReportResponse = {
  report: FiledIssueReport
}

export function fileIssueReport(body: FileIssueReportRequest) {
  return apiClient
    .post('v1/issue-reports', { json: body, timeout: ISSUE_REPORT_FILE_TIMEOUT_MS })
    .json<FileIssueReportResponse>()
}
