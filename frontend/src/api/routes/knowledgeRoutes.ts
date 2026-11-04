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
  // Moments with an offset: what was said from one and before the other.
  from?: string | null
  to?: string | null
  limit?: number
}

// What a document was made from, as the API names it.
export type KnowledgeOrigin =
  | { kind: 'timelineRecord' }
  | { kind: 'learning', learningId: string }
  | { kind: 'companyDoc', section: string }

// Mirrors `DocumentView` in api/src/models/knowledge_document.rs: one document whole.
export type KnowledgeDocument = {
  citation: KnowledgeCitation
  origin: KnowledgeOrigin
  passages: KnowledgePassage[]
  projects: { id: string, name: string }[]
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
