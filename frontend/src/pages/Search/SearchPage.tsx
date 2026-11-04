// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Button, SearchField, Spinner } from '@heroui/react'
import { LuSparkles } from 'react-icons/lu'
import { Page } from '../../layout/page/Page'
import { SearchResultRow } from '../../search/SearchResultRow'

// Misc
import { useInitialFocus } from '../../hooks/useInitialFocus'
import { useAskElysia } from '../../elysia/useAskElysia'
import { searchGroupLabelKeys } from '../../search/searchPresentation'
import { groupSearchResults } from '../../search/searchRanking'
import { useGlobalSearch } from '../../search/useGlobalSearch'
import { SEARCH_QUERY_PARAM } from '../../urls'

// Every result for a query, which the topbar's View all opens: each kind of result as a
// section, closest first, then Ask Elysia. The query lives in the address, so a search can
// be shared, reloaded, and come back to.
export function SearchPage() {
  const { t } = useTranslation([ 'search', 'common' ])
  const [ params, setParams ] = useSearchParams()
  const [ isAsking, setIsAsking ] = useState(false)
  const inputRef = useInitialFocus()
  const askElysia = useAskElysia()

  const query = params.get(SEARCH_QUERY_PARAM) ?? ''
  const trimmedQuery = query.trim()
  const search = useGlobalSearch(query)
  const sections = groupSearchResults(search.results)

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
      // Replacing the entry, so Back leaves the page rather than stepping through each letter.
      onChange={(value) => setParams(
        value
          ? { [SEARCH_QUERY_PARAM]: value }
          : {},
        { replace: true },
      )}
    >
      <SearchField.Group>
        {search.isLoading
          ? <Spinner size='sm' className='ml-3' aria-label={t('results.loading')} />
          : <SearchField.SearchIcon />}
        <SearchField.Input ref={inputRef} placeholder={t('common:search.placeholder')} />
        <SearchField.ClearButton />
      </SearchField.Group>
    </SearchField>

    {trimmedQuery && <div className='max-w-2xl'>
      {search.isLoading && <p role='status' className='relaxed opacity-70'>{t('results.loading')}</p>}
      {!search.isLoading && !sections.length && <p role='status' className='relaxed opacity-70'>{
        t('results.none', { query: trimmedQuery })
      }</p>}
      {sections.map((section) => <section key={section.group} className='relaxed'>
        <h2 className='compact px-3 text-sm font-semibold opacity-70'>{
          t(searchGroupLabelKeys[section.group])
        }</h2>
        <ul>{
          section.results.map((result) => <li key={result.id}>
            <SearchResultRow result={result} />
          </li>)
        }</ul>
      </section>)}
      <Button variant='outline' isPending={isAsking} onPress={() => void ask()}>
        <LuSparkles className='size-4' aria-hidden />
        <span>{t('results.askElysia', { query: trimmedQuery })}</span>
      </Button>
    </div>}
  </Page>
}
