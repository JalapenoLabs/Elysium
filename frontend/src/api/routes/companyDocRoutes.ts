// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// The doc's fixed sections, in the order it is read. Mirrors `Section` in
// api/src/models/company_doc.rs; docs/knowledge.md, The company doc.
export const COMPANY_DOC_SECTIONS = [
  'company',
  'leadership',
  'people',
  'stakeholders',
  'products',
  'projects',
  'glossary',
] as const
export type CompanyDocSectionKey = typeof COMPANY_DOC_SECTIONS[number]

// The most characters a section holds, as `company_doc_sections.content` allows.
export const COMPANY_DOC_SECTION_MAX_CHARACTERS = 12_000

// The agent that acted for whoever proposed a change: an MCP client (`mcp:<client id>`) with the
// name it registered, or a coding session's agent (`session:<number>`).
export type CompanyDocAgent = {
  id: string
  name: string | null
}

// A change proposed to one section, waiting for a person. Mirrors `ProposalResponse` in
// api/src/company_doc/mod.rs.
export type CompanyDocProposal = {
  id: string
  section: CompanyDocSectionKey
  // The section as it read when the change was proposed, and as the change would leave it.
  baseContent: string
  content: string
  reason: string
  quote: string | null
  source: string | null
  timelineRecordId: string | null
  // The review's run that proposed it, while that run exists.
  workflowRunId: string | null
  state: 'pending' | 'approved' | 'rejected'
  decidedBy: string | null
  decidedAt: string | null
  // A person, or the machine user of Elysia or the coding agent.
  createdBy: string
  agent: CompanyDocAgent | null
  createdAt: string
}

export type CompanyDocSection = {
  section: CompanyDocSectionKey
  // Markdown.
  content: string
  updatedAt: string
  // Pending proposals to this section, oldest first.
  proposals: CompanyDocProposal[]
}

// Mirrors `CompanyDocResponse`.
export type CompanyDoc = {
  sections: CompanyDocSection[]
}

// One change to the doc. `kind` is text, so a kind a later API writes still loads; `data` holds
// `changes`, each section's text `{ from, to }`, and `undid` when an undo wrote it.
export type CompanyDocHistoryEntry = {
  id: string
  kind: string
  createdBy: string
  proposalId: string | null
  data: Record<string, unknown>
  createdAt: string
}

// The doc's key, revalidated by `companyDoc.changed`.
export const COMPANY_DOC_KEY = 'v1/company-doc'

// The history's key, revalidated with the doc.
export const COMPANY_DOC_HISTORY_KEY = 'v1/company-doc/history'

export function getCompanyDoc() {
  return apiClient
    .get(COMPANY_DOC_KEY)
    .json<CompanyDoc>()
}

// Sections left out keep their text.
export function editCompanyDoc(sections: Partial<Record<CompanyDocSectionKey, string>>) {
  return apiClient
    .patch(COMPANY_DOC_KEY, { json: { sections }})
    .json<CompanyDoc>()
}

type DecidedResponse = {
  proposal: CompanyDocProposal
  doc: CompanyDoc
}

// A section that changed since merges both changes; one whose lines overlap answers 409 with
// the code `proposal_overlaps`.
export function approveCompanyDocProposal(proposalId: string) {
  return apiClient
    .post(`v1/company-doc/proposals/${proposalId}/approve`)
    .json<DecidedResponse>()
}

export function rejectCompanyDocProposal(proposalId: string) {
  return apiClient
    .post(`v1/company-doc/proposals/${proposalId}/reject`)
    .json<DecidedResponse>()
}

type HistoryResponse = {
  history: CompanyDocHistoryEntry[]
}

// Oldest first.
export function listCompanyDocHistory() {
  return apiClient
    .get(COMPANY_DOC_HISTORY_KEY)
    .json<HistoryResponse>()
}

export function undoCompanyDocChange(entryId: string) {
  return apiClient
    .post(`v1/company-doc/history/${entryId}/undo`)
    .json<CompanyDoc>()
}

type ReviewResponse = {
  run: { id: string, state: string }
  // False when a review was already under way, which is answered instead.
  started: boolean
}

export function startCompanyDocReview() {
  return apiClient
    .post('v1/company-doc/review')
    .json<ReviewResponse>()
}
