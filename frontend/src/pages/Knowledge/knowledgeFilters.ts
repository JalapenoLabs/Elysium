// Copyright © 2026 Jalapeno Labs

import type { KnowledgeSearchQuery } from '../../api/routes/knowledgeRoutes'

// Utility
import { parseDate, toZoned } from '@internationalized/date'

// Misc
import { SEARCH_QUERY_PARAM } from '../../urls'

// The Knowledge page's search and filters, kept in the address so a search can be shared,
// reloaded, and come back to from a document. The API reads instants; the page reads days in
// the viewer's zone, so `toKnowledgeQuery` turns one into the other.
export type KnowledgeFilters = {
  // Source names, as the index names them (`knowledgePresentation.ts`); empty for every source.
  sources: string[]
  projectId: string | null
  personId: string | null
  // Calendar days, `YYYY-MM-DD`, both included: what was said from the start of one to the end
  // of the other.
  fromDay: string | null
  toDay: string | null
}

export const NO_KNOWLEDGE_FILTERS: KnowledgeFilters = {
  sources: [],
  projectId: null,
  personId: null,
  fromDay: null,
  toDay: null,
}

// Each filter's parameter in the address.
const FILTER_PARAMS = {
  sources: 'source',
  projectId: 'project',
  personId: 'person',
  fromDay: 'from',
  toDay: 'to',
} as const satisfies Record<keyof KnowledgeFilters, string>

// A day the address holds, if it is one: anything else in the address is ignored rather
// than sent to the API, which would refuse it.
function readDay(value: string | null) {
  if (!value) {
    return null
  }
  try {
    // A day past its month's end, such as February 30th, parses as the last day there is; it
    // is not the day the address named.
    const day = parseDate(value).toString()
    if (day !== value) {
      console.debug('readKnowledgeFilters ignored a day that does not exist', { value })
      return null
    }
    return day
  }
  catch (error) {
    console.debug('readKnowledgeFilters ignored a day it cannot read', { value, error })
    return null
  }
}

export function readKnowledgeQuery(params: URLSearchParams) {
  return params.get(SEARCH_QUERY_PARAM) ?? ''
}

export function readKnowledgeFilters(params: URLSearchParams): KnowledgeFilters {
  const sources = params.get(FILTER_PARAMS.sources)
    ?.split(',')
    .map((source) => source.trim())
    .filter(Boolean)
    ?? []
  return {
    sources,
    projectId: params.get(FILTER_PARAMS.projectId) || null,
    personId: params.get(FILTER_PARAMS.personId) || null,
    fromDay: readDay(params.get(FILTER_PARAMS.fromDay)),
    toDay: readDay(params.get(FILTER_PARAMS.toDay)),
  }
}

// The address for `query` under `filters`, with nothing for a filter left open, keeping every
// other parameter `current` holds.
export function writeKnowledgeSearch(current: URLSearchParams, query: string, filters: KnowledgeFilters) {
  const next = new URLSearchParams(current)
  const values: [ string, string | null ][] = [
    [ SEARCH_QUERY_PARAM, query ],
    [ FILTER_PARAMS.sources, filters.sources.join(',') ],
    [ FILTER_PARAMS.projectId, filters.projectId ],
    [ FILTER_PARAMS.personId, filters.personId ],
    [ FILTER_PARAMS.fromDay, filters.fromDay ],
    [ FILTER_PARAMS.toDay, filters.toDay ],
  ]
  for (const [ param, value ] of values) {
    if (value) {
      next.set(param, value)
    }
    else {
      next.delete(param)
    }
  }
  return next
}

// How many filters narrow the search, for the filters button.
export function countKnowledgeFilters(filters: KnowledgeFilters) {
  let count = filters.sources.length
  for (const value of [ filters.projectId, filters.personId, filters.fromDay, filters.toDay ]) {
    if (value) {
      count += 1
    }
  }
  return count
}

// What the API is asked for: the days as instants in the viewer's zone, the last day lasting
// until it ends, so a period from November 1st to November 1st is that whole day.
export function toKnowledgeQuery(
  query: string,
  filters: KnowledgeFilters,
  timeZone: string,
  limit?: number,
): KnowledgeSearchQuery {
  return {
    q: query,
    sources: filters.sources,
    projectId: filters.projectId,
    personId: filters.personId,
    from: filters.fromDay
      ? toZoned(parseDate(filters.fromDay), timeZone).toAbsoluteString()
      : null,
    to: filters.toDay
      ? toZoned(parseDate(filters.toDay).add({ days: 1 }), timeZone).toAbsoluteString()
      : null,
    limit,
  }
}
