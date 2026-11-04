// Copyright © 2026 Jalapeno Labs

import type { PageTab } from '../../components/PageTabs'
import type { SearchGroup } from '../../search/searchTypes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Button, SearchField, Spinner, Tabs } from '@heroui/react'
import { LuSparkles } from 'react-icons/lu'
import { PageTabs } from '../../components/PageTabs'
import { Page } from '../../layout/page/Page'
import { SearchResultRow } from '../../search/SearchResultRow'

// Misc
import { RECORD_SEARCH_PAGE_LIMIT } from '../../constants'
import { useAskElysia } from '../../elysia/useAskElysia'
import { useFindHotkey } from '../../hooks/useFindHotkey'
import { useTabParam } from '../../hooks/useTabParam'
import { searchGroupLabelKeys } from '../../search/searchPresentation'
import { groupSearchResults } from '../../search/searchRanking'
import { SEARCH_GROUPS } from '../../search/searchTypes'
import { useGlobalSearch } from '../../search/useGlobalSearch'
import { SEARCH_QUERY_PARAM } from '../../urls'

// Every kind of result together, the first and default tab, or one kind alone.
type KindTab = 'all' | SearchGroup
const KIND_TABS: readonly KindTab[] = [ 'all', ...SEARCH_GROUPS ]

// Every result for a query, which the topbar's View all opens: a tab for all of them and one
// for each kind, each with its count, then Ask Elysia. All shows each kind as a section,
// closest first. The query and the tab live in the address, so a search can be shared,
// reloaded, and come back to.
export function SearchPage() {
  const { t } = useTranslation([ 'search', 'common' ])
  const [ params, setParams ] = useSearchParams()
  const [ kind, selectKind ] = useTabParam(KIND_TABS)
  const [ isAsking, setIsAsking ] = useState(false)
  // The page is its search, so typing starts there, and Ctrl+F comes back to it.
  const inputRef = useFindHotkey(true)
  const askElysia = useAskElysia()

  const query = params.get(SEARCH_QUERY_PARAM) ?? ''
  const trimmedQuery = query.trim()
  const search = useGlobalSearch(query, RECORD_SEARCH_PAGE_LIMIT)
  const sections = groupSearchResults(search.results)

  const countByKind: Record<KindTab, number> = {
    all: search.results.length,
    pages: 0,
    actions: 0,
    actionItems: 0,
    projects: 0,
    codingSessions: 0,
    initiatives: 0,
  }
  for (const section of sections) {
    countByKind[section.group] = section.results.length
  }
  const tabs: PageTab<KindTab>[] = KIND_TABS.map((tab) => ({
    id: tab,
    label: tab === 'all'
      ? t('page.all')
      : t(searchGroupLabelKeys[tab]),
    // Unknown until the API's records arrive, so no count reads low while it answers.
    count: search.isLoading
      ? null
      : countByKind[tab],
  }))
  const shownSections = kind === 'all'
    ? sections
    : sections.filter((section) => section.group === kind)

  async function ask() {
    setIsAsking(true)
    await askElysia(trimmedQuery)
    setIsAsking(false)
  }

  return <Page title={t('page.title')}>
    <SearchField
      id='search-page-query'
      autoComplete='off'
      aria-label={t('common:search.label')}
      className='relaxed w-full max-w-2xl'
      value={query}
      // Replacing the entry, so Back leaves the page rather than stepping through each letter,
      // and keeping the open tab.
      onChange={(value) => setParams((current) => {
        const next = new URLSearchParams(current)
        if (value) {
          next.set(SEARCH_QUERY_PARAM, value)
        }
        else {
          next.delete(SEARCH_QUERY_PARAM)
        }
        return next
      }, { replace: true })}
    >
      <SearchField.Group>
        {search.isLoading
          ? <Spinner size='sm' className='ml-3' aria-label={t('results.loading')} />
          : <SearchField.SearchIcon />}
        <SearchField.Input ref={inputRef} placeholder={t('common:search.placeholder')} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>

    {trimmedQuery && <PageTabs
      label={t('page.kinds')}
      tabs={tabs}
      selectedKey={kind}
      onSelectionChange={selectKind}
    >
      <Tabs.Panel id={kind} className='max-w-2xl'>
        {search.isLoading && <p role='status' className='relaxed opacity-70'>{t('results.loading')}</p>}
        {!search.isLoading && search.hasFailed && <p role='status' className='relaxed text-warning'>{
          t('results.failed')
        }</p>}
        {!search.isLoading && !search.hasFailed && !shownSections.length && <p
          role='status'
          className='relaxed opacity-70'
        >{
          t('results.none', { query: trimmedQuery })
        }</p>}
        {shownSections.map((section) => <section key={section.group} className='relaxed'>
          {kind === 'all' && <h2 className='compact px-3 text-sm font-semibold opacity-70'>{
            t(searchGroupLabelKeys[section.group])
          }</h2>}
          <ul aria-label={t(searchGroupLabelKeys[section.group])}>{
            section.results.map((result) => <li key={result.id}>
              <SearchResultRow result={result} />
            </li>)
          }</ul>
        </section>)}
        <Button variant='outline' isPending={isAsking} onPress={() => void ask()}>
          <LuSparkles className='size-4' aria-hidden />
          <span>{t('results.askElysia', { query: trimmedQuery })}</span>
        </Button>
      </Tabs.Panel>
    </PageTabs>}
  </Page>
}
