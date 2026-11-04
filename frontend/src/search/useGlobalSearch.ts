// Copyright © 2026 Jalapeno Labs

import type { SearchActionContext, SearchSourceAnswer, SearchViewer } from './searchTypes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsAdmin, selectIsCompanyMember, selectMe } from '../store/authSlice'
import { useAppDispatch, useAppSelector } from '../store/hooks'

// Misc
import { RECORD_SEARCH_TOPBAR_LIMIT } from '../constants'
import { useConfirm } from '../hooks/useConfirm'
import { TRANSLATION_NAMESPACES } from '../i18n'
import { findRegistryResults, prepareSearchEntries } from './registrySearch'
import { registeredSearchEntries } from './searchRegistry'
import { rankSearchResults } from './searchRanking'
import { useRecordSearch } from './useRecordSearch'

// Everything search finds for the query, closest first, from every source. A source is a
// hook that answers for the query with its results, whether it is still loading, and whether
// it could not answer. The registry's pages and actions answer at once, in the browser; the
// API's records (`useRecordSearch`) answer 300ms after typing stops, `recordLimit` of each
// kind. The topbar and the search page show every source's results, loading, and failure
// alike.
export function useGlobalSearch(query: string, recordLimit = RECORD_SEARCH_TOPBAR_LIMIT) {
  const registry = useRegistrySearch(query)
  const records = useRecordSearch(query, recordLimit)

  return useMemo(() => ({
    results: rankSearchResults([ ...registry.results, ...records.results ]),
    isLoading: registry.isLoading || records.isLoading,
    hasFailed: registry.hasFailed || records.hasFailed,
  }), [ registry, records ])
}

// The pages and actions registered beside each part of the app (`searchRegistry.ts`), in the
// viewer's language, limited to what their role opens and what applies to their account.
function useRegistrySearch(query: string): SearchSourceAnswer {
  const { t } = useTranslation(TRANSLATION_NAMESPACES)
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const isAdmin = useAppSelector(selectIsAdmin)
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  // A session that signed in with a second factor: with Kratos asking for the highest factor
  // a person has (`required_aal: highest_available`), that is someone with 2FA on.
  const hasTwoFactor = Boolean(useAppSelector(selectMe)?.hasSecondFactor)

  const prepared = useMemo(() => {
    const viewer: SearchViewer = {
      audiences: {
        everyone: true,
        members: isCompanyMember,
        admins: isAdmin,
      },
      conditions: {
        twoFactorOn: hasTwoFactor,
        twoFactorOff: !hasTwoFactor,
      },
    }
    return prepareSearchEntries(registeredSearchEntries, t, viewer)
  }, [ t, isAdmin, isCompanyMember, hasTwoFactor ])

  return useMemo(() => {
    const context: SearchActionContext = { dispatch, confirm }
    return {
      results: findRegistryResults(prepared, query, context),
      isLoading: false,
      hasFailed: false,
    }
  }, [ prepared, query, dispatch, confirm ])
}
