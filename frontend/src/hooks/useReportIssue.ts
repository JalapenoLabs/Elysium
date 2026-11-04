// Copyright © 2026 Jalapeno Labs

// Core
import { useContext } from 'react'

// Misc
import { ReportIssueContext } from '../gates/ReportIssueGate'

// Opens the app's report dialog for a problem. See `IssueReportSubject` in
// src/api/routes/issueReportRoutes.ts for what can be reported.
export function useReportIssue() {
  return useContext(ReportIssueContext)
}
