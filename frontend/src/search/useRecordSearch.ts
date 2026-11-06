// Copyright © 2026 Jalapeno Labs

import type { SearchSourceAnswer } from './searchTypes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Misc
import { searchRecords } from '../api/routes/searchRoutes'
import { RECORD_SEARCH_MAX_CHARACTERS } from '../constants'
import { TRANSLATION_NAMESPACES } from '../i18n'
import { toRecordResults } from './recordSearch'
import { toSearchWords } from './searchMatch'
import { useDebouncedRequest } from './useDebouncedRequest'

// The action items, projects, coding sessions, initiatives, and pages whose titles match the query,
// up to `limit` of each kind, from `GET /api/v1/search`, asked 300ms after typing stops and
// cancelled when a newer query replaces it (`useDebouncedRequest`).
export function useRecordSearch(query: string, limit: number): SearchSourceAnswer {
  const { t } = useTranslation(TRANSLATION_NAMESPACES)

  const trimmedQuery = query.trim()
  // Only what the API would search: something with words in it, and short enough. Anything
  // else finds no records, as the registry finds no pages for it.
  const isSearchable = Boolean(toSearchWords(trimmedQuery).length)
    && trimmedQuery.length <= RECORD_SEARCH_MAX_CHARACTERS
  const { answer, isLoading, hasFailed } = useDebouncedRequest(
    isSearchable
      ? JSON.stringify([ trimmedQuery, limit ])
      : null,
    (signal) => searchRecords(trimmedQuery, limit, signal),
  )

  return useMemo(() => ({
    results: answer
      ? toRecordResults(answer.results, trimmedQuery, t)
      : [],
    isLoading,
    hasFailed,
  }), [ answer, trimmedQuery, isLoading, hasFailed, t ])
}
