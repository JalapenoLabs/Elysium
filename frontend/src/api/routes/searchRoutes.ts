// Copyright © 2026 Jalapeno Labs

import type { ActionItemState } from './actionItemRoutes'
import type { InitiativeState } from './initiativeRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `SearchHit` in api/src/routes/v1/search/search_records.rs: one record whose title
// matched, carrying what a row of results shows and no more.
export type RecordSearchHit =
  | { kind: 'actionItem', id: string, title: string, state: ActionItemState }
  | { kind: 'project', id: string, title: string }
  | { kind: 'codingSession', id: number, title: string }
  | { kind: 'initiative', id: string, title: string, state: InitiativeState }
  // How many edits agents suggested to the page wait for a person.
  | { kind: 'page', id: string, title: string, pendingSuggestions: number }

export type RecordSearchKind = RecordSearchHit['kind']

type SearchRecordsResponse = {
  // The closest `limit` of each kind, each kind in its own order.
  results: RecordSearchHit[]
}

// The action items, projects, coding sessions, initiatives, and pages whose titles match `query`.
// Never retried: a newer keystroke asks again, and a retried older search could otherwise
// answer after it. `signal` cancels it once a newer query replaces it.
export function searchRecords(query: string, limit: number, signal: AbortSignal) {
  return apiClient
    .get('v1/search', {
      searchParams: { q: query, limit },
      signal,
      retry: 0,
    })
    .json<SearchRecordsResponse>()
}
