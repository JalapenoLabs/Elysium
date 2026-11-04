// Copyright © 2026 Jalapeno Labs

import type { KnowledgeFilters } from './knowledgeFilters'

// Utility
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { searchKnowledge } from '../../api/routes/knowledgeRoutes'
import { KNOWLEDGE_SEARCH_MAX_CHARACTERS } from '../../constants'
import { useDebouncedRequest } from '../../search/useDebouncedRequest'
import { toKnowledgeQuery } from './knowledgeFilters'

// The knowledge base's answer for `query` under `filters`, `limit` documents at most, from
// `GET /api/v1/knowledge/search`: asked 300ms after typing or filtering stops and cancelled when
// a newer search replaces it (`useDebouncedRequest`). `isEnabled` keeps it from asking for
// someone the API would refuse, such as a guest. A blank query, or one longer than the API
// searches, asks nothing.
export function useKnowledgeSearch(
  query: string,
  filters: KnowledgeFilters,
  limit: number,
  isEnabled = true,
) {
  const trimmedQuery = query.trim()
  const isSearchable = isEnabled
    && Boolean(trimmedQuery)
    && trimmedQuery.length <= KNOWLEDGE_SEARCH_MAX_CHARACTERS
  const request = toKnowledgeQuery(trimmedQuery, filters, getLocalTimeZone(), limit)

  return useDebouncedRequest(
    isSearchable
      ? JSON.stringify(request)
      : null,
    (signal) => searchKnowledge(request, signal),
  )
}
