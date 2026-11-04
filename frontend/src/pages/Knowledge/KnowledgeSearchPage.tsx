// Copyright © 2026 Jalapeno Labs

import type { KnowledgeFilters } from './knowledgeFilters'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Badge, Button, SearchField, Spinner, Tooltip } from '@heroui/react'
import { LuListFilter } from 'react-icons/lu'
import { KnowledgeFiltersDrawer } from './KnowledgeFiltersDrawer'
import { KnowledgeFilterTags } from './KnowledgeFilterTags'
import { KnowledgeResultCard } from './KnowledgeResultCard'

// Misc
import { KNOWLEDGE_SEARCH_PAGE_LIMIT } from '../../constants'
import { formatNumber } from '../../format/numbers'
import { useFindHotkey } from '../../hooks/useFindHotkey'
import { useProjectsLoader, useUsersLoader } from '../../hooks/useServerData'
import {
  countKnowledgeFilters,
  readKnowledgeFilters,
  readKnowledgeQuery,
  writeKnowledgeSearch,
} from './knowledgeFilters'
import { useKnowledgeSearch } from './useKnowledgeSearch'

// The Knowledge page's search: one field, the filters button beside it, the filters in force
// as tags, then the documents found, best first, each with its best passages. The query and the
// filters live in the address, so a search can be shared, reloaded, and come back to from a
// document. When the embedding model cannot answer, the search runs by its words alone and says
// so above the results.
export function KnowledgeSearchPage() {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const [ params, setParams ] = useSearchParams()
  // The page is its search, so typing starts there, and Ctrl+F comes back to it.
  const inputRef = useFindHotkey(true)
  // Opening the drawer remounts it, so it always starts from the filters in force.
  const [ filtersOpenedTimes, setFiltersOpenedTimes ] = useState(0)
  const [ isFiltering, setIsFiltering ] = useState(false)
  // The filters and the tags name projects and people.
  useProjectsLoader()
  useUsersLoader()

  const query = readKnowledgeQuery(params)
  const filters = useMemo(() => readKnowledgeFilters(params), [ params ])
  const trimmedQuery = query.trim()
  const search = useKnowledgeSearch(query, filters, KNOWLEDGE_SEARCH_PAGE_LIMIT)
  const filterCount = countKnowledgeFilters(filters)

  // Replacing the entry, so Back leaves the page rather than stepping through each letter.
  function show(nextQuery: string, nextFilters: KnowledgeFilters) {
    setParams((current) => writeKnowledgeSearch(current, nextQuery, nextFilters), { replace: true })
  }

  const filtersLabel = filterCount
    ? t('filters.openActive', { count: filterCount })
    : t('filters.open')

  return <>
    <div className='relaxed flex max-w-3xl items-center gap-2'>
      <SearchField
        id='knowledge-search-query'
        autoComplete='off'
        aria-label={t('search.label')}
        className='w-full'
        value={query}
        onChange={(value) => show(value, filters)}
      >
        <SearchField.Group>
          {search.isLoading
            ? <Spinner size='sm' className='ml-3' aria-label={t('search.loading')} />
            : <SearchField.SearchIcon />}
          <SearchField.Input ref={inputRef} placeholder={t('search.placeholder')} />
          <SearchField.ClearButton />
        </SearchField.Group>
      </SearchField>
      <Badge.Anchor>
        <Tooltip>
          <Button
            isIconOnly
            variant='outline'
            aria-label={filtersLabel}
            onPress={() => {
              setFiltersOpenedTimes((times) => times + 1)
              setIsFiltering(true)
            }}
          >
            <LuListFilter className='size-4' aria-hidden />
          </Button>
          <Tooltip.Content>{filtersLabel}</Tooltip.Content>
        </Tooltip>
        {Boolean(filterCount) && <Badge
          aria-hidden
          color='accent'
          size='sm'
          className='pointer-events-none'
        >{
            formatNumber(filterCount)
          }</Badge>}
      </Badge.Anchor>
    </div>

    <KnowledgeFilterTags filters={filters} onChange={(nextFilters) => show(query, nextFilters)} />

    {trimmedQuery && <section aria-label={t('search.results')} className='max-w-3xl'>
      {search.isLoading && <p role='status' className='relaxed opacity-70'>{t('search.loading')}</p>}
      {!search.isLoading && search.hasFailed && <p role='status' className='relaxed text-warning'>{
        t('search.failed')
      }</p>}
      {search.answer && !search.answer.searchedByMeaning && <p className='relaxed text-sm opacity-70'>{
        t('search.wordsOnly')
      }</p>}
      {search.answer && !search.answer.results.length && <p role='status' className='relaxed opacity-70'>{
        t('search.none', { query: trimmedQuery })
      }</p>}
      {search.answer?.results.map((result) => <KnowledgeResultCard
        key={result.citation.documentId}
        result={result}
      />)}
    </section>}

    <KnowledgeFiltersDrawer
      key={filtersOpenedTimes}
      isOpen={isFiltering}
      onOpenChange={setIsFiltering}
      filters={filters}
      onApply={(nextFilters) => show(query, nextFilters)}
    />
  </>
}
