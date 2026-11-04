// Copyright © 2026 Jalapeno Labs

import type { SearchSourceAnswer } from './searchTypes'

// Core
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectIsCompanyMember } from '../store/authSlice'
import { useAppSelector } from '../store/hooks'

// Misc
import { NO_KNOWLEDGE_FILTERS } from '../pages/Knowledge/knowledgeFilters'
import { useKnowledgeSearch } from '../pages/Knowledge/useKnowledgeSearch'
import { toKnowledgeResults } from './knowledgeSearch'

// The knowledge base's documents for the query, up to `limit`, from the knowledge search, as the
// Knowledge page asks it with no filters. Only the company's members search the knowledge base
// (docs/knowledge.md, Who sees what), so for anyone else this source asks nothing and finds
// nothing, as the sidebar leaves out the Knowledge page for them.
export function useKnowledgeResults(query: string, limit: number): SearchSourceAnswer {
  const { t } = useTranslation('knowledge')
  const isCompanyMember = useAppSelector(selectIsCompanyMember)
  const { answer, isLoading, hasFailed } = useKnowledgeSearch(query, NO_KNOWLEDGE_FILTERS, limit, isCompanyMember)

  return useMemo(() => ({
    results: answer
      ? toKnowledgeResults(answer.results, query.trim(), t)
      : [],
    isLoading,
    hasFailed,
  }), [ answer, query, isLoading, hasFailed, t ])
}
