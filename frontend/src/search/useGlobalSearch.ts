// Copyright © 2026 Jalapeno Labs

import type { SearchActionContext, SearchAudience, SearchSourceAnswer } from './searchTypes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin, selectIsCompanyMember } from '../store/authSlice'
import { useAppDispatch, useAppSelector } from '../store/hooks'

// Misc
import { useConfirm } from '../hooks/useConfirm'
import { TRANSLATION_NAMESPACES } from '../i18n'
import { findRegistryResults, prepareSearchEntries } from './registrySearch'
import { registeredSearchEntries } from './searchRegistry'
import { rankSearchResults } from './searchRanking'

// Everything search finds for the query, closest first, from every source. A source is a
// hook that answers for the query with its results and whether it is still loading. The
// registry's pages and actions answer at once, in the browser; a source that asks the API
// joins below it, waiting 300ms after typing stops and reporting `isLoading` meanwhile, and
// the topbar and the search page show its results, its loading, and its section unchanged.
export function useGlobalSearch(query: string) {
  const registry = useRegistrySearch(query)

  return useMemo(() => ({
    results: rankSearchResults(registry.results),
    isLoading: registry.isLoading,
  }), [ registry ])
}

// The pages and actions registered beside each part of the app (`searchRegistry.ts`), in the
// viewer's language, limited to what their role opens.
function useRegistrySearch(query: string): SearchSourceAnswer {
  const { t } = useTranslation(TRANSLATION_NAMESPACES)
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const isAdmin = useAppSelector(selectIsAdmin)
  const isCompanyMember = useAppSelector(selectIsCompanyMember)

  const prepared = useMemo(() => {
    const canSee = {
      everyone: true,
      members: isCompanyMember,
      admins: isAdmin,
    } as const satisfies Record<SearchAudience, boolean>
    return prepareSearchEntries(registeredSearchEntries, t, canSee)
  }, [ t, isAdmin, isCompanyMember ])

  return useMemo(() => {
    const context: SearchActionContext = { dispatch, confirm }
    return {
      results: findRegistryResults(prepared, query, context),
      isLoading: false,
    }
  }, [ prepared, query, dispatch, confirm ])
}
