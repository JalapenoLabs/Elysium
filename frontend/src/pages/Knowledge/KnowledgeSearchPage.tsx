// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { KnowledgeFilters, KnowledgeScope } from './knowledgeFilters'

// Core
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Badge, Button, SearchField, Spinner, Tabs, Tooltip } from '@heroui/react'
import { LuListFilter } from 'react-icons/lu'
import { PageTabs } from '../../components/PageTabs'
import { Page } from '../../layout/page/Page'
import { KnowledgeFiltersDrawer } from './KnowledgeFiltersDrawer'
import { KnowledgeFilterTags } from './KnowledgeFilterTags'
import { KnowledgeResultCard } from './KnowledgeResultCard'

// Misc
import { KNOWLEDGE_SEARCH_PAGE_LIMIT } from '../../constants'
import { formatNumber } from '../../format/numbers'
import { useFindHotkey } from '../../hooks/useFindHotkey'
import { useProjectsLoader, useUsersLoader } from '../../hooks/useServerData'
import { useTabParam } from '../../hooks/useTabParam'
import { UrlTree } from '../../urls'
import {
  countKnowledgeFilters,
  KNOWLEDGE_SCOPES,
  readKnowledgeFilters,
  readKnowledgeQuery,
  readSearchedSources,
  scopeKnowledgeFilters,
  writeKnowledgeSearch,
} from './knowledgeFilters'
import { useKnowledgeSearch } from './useKnowledgeSearch'

type ScopeWords = {
  tab: ParseKeys<'knowledge'>
  placeholder: ParseKeys<'knowledge'>
  none: ParseKeys<'knowledge'>
}

const scopeWords = {
  everything: {
    tab: 'search.everything',
    placeholder: 'search.placeholder',
    none: 'search.none',
  },
  pages: {
    tab: 'search.pages',
    placeholder: 'search.placeholderPages',
    none: 'search.nonePages',
  },
} as const satisfies Record<KnowledgeScope, ScopeWords>

// `/knowledge/search`, the knowledge base's search: one field, the filters button beside it, a tab
// for everything and one for pages alone, the filters in force as tags, then the documents found,
// best first, each with its best passages. Both tabs are one search under other sources, so they
// rank alike and spend one budget. The query, the tab, and the filters live in the address, so a
// search can be shared, reloaded, and come back to from a document. When the embedding model cannot
// answer, the search runs by its words alone and says so above the results.
export function KnowledgeSearchPage() {
  const { t } = useTranslation([ 'knowledge', 'common' ])
  const [ params, setParams ] = useSearchParams()
  // The page is its search, so typing starts there, and Ctrl+F comes back to it.
  const inputRef = useFindHotkey(true)
  // Opening the drawer remounts it, so it always starts from the filters in force.
  const [ filtersOpenedTimes, setFiltersOpenedTimes ] = useState(0)
  const [ isFiltering, setIsFiltering ] = useState(false)
  const [ scope, selectScope ] = useTabParam(KNOWLEDGE_SCOPES)
  // The filters and the tags name projects and people.
  useProjectsLoader()
  useUsersLoader()

  const query = readKnowledgeQuery(params)
  const filters = useMemo(() => scopeKnowledgeFilters(readKnowledgeFilters(params), scope), [ params, scope ])
  const trimmedQuery = query.trim()
  const search = useKnowledgeSearch(
    query,
    { ...filters, sources: readSearchedSources(filters, scope) },
    KNOWLEDGE_SEARCH_PAGE_LIMIT,
  )
  const filterCount = countKnowledgeFilters(filters)

  // Replacing the entry, so Back leaves the page rather than stepping through each letter.
  function show(nextQuery: string, nextFilters: KnowledgeFilters) {
    setParams((current) => writeKnowledgeSearch(current, nextQuery, nextFilters), { replace: true })
  }

  const filtersLabel = filterCount
    ? t('filters.openActive', { count: filterCount })
    : t('filters.open')

  return <Page
    title={t('sections.search')}
    parents={[{ label: t('title'), href: UrlTree.knowledge }]}
  >
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
          <SearchField.Input ref={inputRef} placeholder={t(scopeWords[scope].placeholder)} />
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

    <PageTabs
      label={t('search.scopes')}
      tabs={KNOWLEDGE_SCOPES.map((tab) => ({
        id: tab,
        label: t(scopeWords[tab].tab),
        // Counting the other tab's results would take a search of its own.
        count: null,
      }))}
      selectedKey={scope}
      onSelectionChange={selectScope}
    >
      <Tabs.Panel id={scope} className='mt-0 p-0'>
        <KnowledgeFilterTags
          scope={scope}
          filters={filters}
          onChange={(nextFilters) => show(query, nextFilters)}
        />

        {trimmedQuery && <section aria-label={t('search.results')} className='max-w-3xl'>
          {search.isLoading && <p role='status' className='relaxed opacity-70'>{t('search.loading')}</p>}
          {!search.isLoading && search.hasFailed && <p role='status' className='relaxed text-warning'>{
            t('search.failed')
          }</p>}
          {search.answer && !search.answer.searchedByMeaning && <p className='relaxed text-sm opacity-70'>{
            t('search.wordsOnly')
          }</p>}
          {search.answer && !search.answer.results.length && <p role='status' className='relaxed opacity-70'>{
            t(scopeWords[scope].none, { query: trimmedQuery })
          }</p>}
          {search.answer?.results.map((result) => <KnowledgeResultCard
            key={result.citation.documentId}
            result={result}
            query={trimmedQuery}
          />)}
        </section>}
      </Tabs.Panel>
    </PageTabs>

    <KnowledgeFiltersDrawer
      key={filtersOpenedTimes}
      scope={scope}
      isOpen={isFiltering}
      onOpenChange={setIsFiltering}
      filters={filters}
      onApply={(nextFilters) => show(query, nextFilters)}
    />
  </Page>
}
