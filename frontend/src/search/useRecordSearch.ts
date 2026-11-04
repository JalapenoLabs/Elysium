// Copyright © 2026 Jalapeno Labs

import type { RecordSearchHit } from '../api/routes/searchRoutes'
import type { SearchSourceAnswer } from './searchTypes'

// Core
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Misc
import { searchRecords } from '../api/routes/searchRoutes'
import { RECORD_SEARCH_DELAY_MS, RECORD_SEARCH_MAX_CHARACTERS } from '../constants'
import { TRANSLATION_NAMESPACES } from '../i18n'
import { toRecordResults } from './recordSearch'
import { toSearchWords } from './searchMatch'

// What the API last answered, and for which search, so an answer is only ever shown for the
// query and limit it was asked for.
type Answer = {
  query: string
  limit: number
  hits: RecordSearchHit[]
  hasFailed: boolean
}

const NOTHING_TO_SEARCH: SearchSourceAnswer = { results: [], isLoading: false, hasFailed: false }

// The action items, projects, coding sessions, and initiatives whose titles match the query,
// up to `limit` of each kind, from `GET /api/v1/search`. It asks 300ms after typing stops
// (ux.md, Search inputs) and cancels a request a newer query replaces, so a slow answer to an
// older query can never land over a newer one. From the keystroke until the answer for that
// query arrives it reports `isLoading`, rather than showing an older query's records.
export function useRecordSearch(query: string, limit: number): SearchSourceAnswer {
  const { t } = useTranslation(TRANSLATION_NAMESPACES)
  const [ answer, setAnswer ] = useState<Answer | null>(null)

  const trimmedQuery = query.trim()
  // Only what the API would search: something with words in it, and short enough. Anything
  // else finds no records, as the registry finds no pages for it.
  const isSearchable = Boolean(toSearchWords(trimmedQuery).length)
    && trimmedQuery.length <= RECORD_SEARCH_MAX_CHARACTERS

  useEffect(() => {
    // Nothing to ask for; the answer below says so without a request.
    if (!isSearchable) {
      return undefined
    }

    const controller = new AbortController()
    const timer = window.setTimeout(async () => {
      try {
        const response = await searchRecords(trimmedQuery, limit, controller.signal)
        // An answer that arrived as a newer query replaced it must not land over that one's.
        if (controller.signal.aborted) {
          return
        }
        setAnswer({ query: trimmedQuery, limit, hits: response.results, hasFailed: false })
      }
      catch (error) {
        // A newer query replaced this one; its own request is already on the way.
        if (controller.signal.aborted) {
          return
        }
        console.debug('useRecordSearch could not search records', { error })
        setAnswer({ query: trimmedQuery, limit, hits: [], hasFailed: true })
      }
    }, RECORD_SEARCH_DELAY_MS)

    return () => {
      window.clearTimeout(timer)
      controller.abort()
    }
  }, [ trimmedQuery, limit, isSearchable ])

  return useMemo(() => {
    if (!isSearchable) {
      return NOTHING_TO_SEARCH
    }

    const isAnswered = answer?.query === trimmedQuery && answer.limit === limit
    if (!isAnswered) {
      return { results: [], isLoading: true, hasFailed: false }
    }
    return {
      results: toRecordResults(answer.hits, trimmedQuery, t),
      isLoading: false,
      hasFailed: answer.hasFailed,
    }
  }, [ answer, trimmedQuery, limit, isSearchable, t ])
}
