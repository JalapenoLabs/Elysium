// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `Person` in api/src/models/knowledge_document.rs: whose record, learning, or edit a
// document came from.
export type KnowledgePerson = {
  id: string
  name: string
}

// How a document is named and linked to wherever it is cited.
export type KnowledgeCitation = {
  documentId: string
  // The source filter's name for it, such as `slack`, `learning`, or `company_doc`.
  source: string
  title: string
  // Where it lives outside Elysium, when it does.
  url: string | null
  occurredAt: string
  person: KnowledgePerson
}

export type KnowledgePassage = {
  position: number
  content: string
}

// Another document holding the same text: two people's connectors read the same conversation.
export type KnowledgeCopy = {
  documentId: string
  person: KnowledgePerson
}

// Mirrors `SearchResult` in api/src/knowledge/search.rs: one document found, with its best
// passages, best first.
export type KnowledgeResult = {
  citation: KnowledgeCitation
  passages: KnowledgePassage[]
  alsoRecordedBy: KnowledgeCopy[]
  score: number
}

export type KnowledgeSearchAnswer = {
  results: KnowledgeResult[]
  // False while the embedding model cannot embed the query: it was searched by its words alone.
  searchedByMeaning: boolean
}

// What a search narrows to; each left out narrows nothing.
export type KnowledgeSearchQuery = {
  q: string
  sources?: string[]
  projectId?: string | null
  personId?: string | null
  // Only pages carrying this label.
  label?: string | null
  // Moments with an offset: what was said from one and before the other.
  from?: string | null
  to?: string | null
  limit?: number
}

// What a document was made from, as the API names it.
export type KnowledgeOrigin =
  | { kind: 'timelineRecord' }
  | { kind: 'learning', learningId: string }
  | { kind: 'companyDoc', section: string, pageId: string }
  | { kind: 'page', pageId: string }

// Mirrors `DocumentView` in api/src/models/knowledge_document.rs: one document whole.
export type KnowledgeDocument = {
  citation: KnowledgeCitation
  origin: KnowledgeOrigin
  passages: KnowledgePassage[]
  // How many passages the document has; the page asks for all of them, so it is the same.
  passageCount: number
  projects: { id: string, name: string }[]
  // The labels its page carries; none for any other source.
  labels: string[]
  alsoRecordedBy: KnowledgeCopy[]
}

// The documents whose passages match the query by their words or their meaning, best first.
// Never retried: a newer keystroke asks again, and a retried older search could otherwise
// answer after it. `signal` cancels it once a newer search replaces it.
export function searchKnowledge(query: KnowledgeSearchQuery, signal: AbortSignal) {
  const searchParams = new URLSearchParams({ q: query.q })
  if (query.sources?.length) {
    searchParams.set('source', query.sources.join(','))
  }
  if (query.projectId) {
    searchParams.set('project', query.projectId)
  }
  if (query.personId) {
    searchParams.set('person', query.personId)
  }
  if (query.label) {
    searchParams.set('label', query.label)
  }
  if (query.from) {
    searchParams.set('from', query.from)
  }
  if (query.to) {
    searchParams.set('to', query.to)
  }
  if (query.limit) {
    searchParams.set('limit', String(query.limit))
  }
  return apiClient
    .get('v1/knowledge/search', { searchParams, signal, retry: 0 })
    .json<KnowledgeSearchAnswer>()
}

type GetKnowledgeDocumentResponse = {
  document: KnowledgeDocument
}

export function getKnowledgeDocument(documentId: string) {
  return apiClient
    .get(`v1/knowledge/documents/${documentId}`)
    .json<GetKnowledgeDocumentResponse>()
}

// Where a question asked of the knowledge base stands.
export type KnowledgeAskState = 'queued' | 'running' | 'answered' | 'failed'

// One passage an answer cites, numbered in the order the answer first cites it.
export type KnowledgeAnswerSource = {
  number: number
  citation: KnowledgeCitation
  // The passage's place in its document, which the document's page opens at.
  position: number
}

// One statement of an answer, never without a source it cites by number.
export type KnowledgeAnswerClaim = {
  text: string
  sources: number[]
}

// Mirrors `Answer` in api/src/models/knowledge_ask.rs: every claim cites a passage the search
// offered; what cited nothing was left out and counted.
export type KnowledgeAnswer = {
  claims: KnowledgeAnswerClaim[]
  sources: KnowledgeAnswerSource[]
  unsupportedClaims: number
}

// Mirrors `KnowledgeAskResponse` in api/src/routes/v1/knowledge/asks/mod.rs: one of the
// person's own questions, as it stands.
export type KnowledgeAsk = {
  id: string
  question: string
  // What the search was narrowed to, each only when asked for.
  filters: {
    sources?: string[]
    project?: string
    person?: string
    from?: string
    to?: string
  }
  state: KnowledgeAskState
  answer: KnowledgeAnswer | null
  error: string | null
  searchedByMeaning: boolean
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
}

type KnowledgeAskResponse = {
  ask: KnowledgeAsk
}

type ListKnowledgeAsksResponse = {
  asks: KnowledgeAsk[]
}

// The person's newest questions, newest first.
export function listKnowledgeAsks() {
  return apiClient
    .get('v1/knowledge/asks')
    .json<ListKnowledgeAsksResponse>()
}

export function getKnowledgeAsk(askId: string) {
  return apiClient
    .get(`v1/knowledge/asks/${askId}`)
    .json<KnowledgeAskResponse>()
}

// Asks a question. Its answer is written afterwards, so the ask comes back queued (or answered
// at once when the search found nothing), and `knowledgeAsk.changed` says when it moves on.
// Never retried: a retry would ask, and spend, twice.
export function askKnowledge(question: string) {
  return apiClient
    .post('v1/knowledge/asks', { json: { question }, retry: 0 })
    .json<KnowledgeAskResponse>()
}
