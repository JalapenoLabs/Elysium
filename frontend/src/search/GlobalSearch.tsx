// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { FocusEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useInteractOutside } from 'react-aria'
import { useTranslation } from 'react-i18next'

// User interface
import { Description, Header, Kbd, Label, Menu, SearchField, Separator, Spinner } from '@heroui/react'
import { Autocomplete } from 'react-aria-components'
import { LuArrowRight, LuSparkles } from 'react-icons/lu'

// Misc
import { useAskElysia } from '../elysia/useAskElysia'
import { isModalOpen } from '../layout/page/pageShortcuts'
import { getSearchUrl } from '../urls'
import { searchGroupLabelKeys } from './searchPresentation'
import { groupSearchResults } from './searchRanking'
import { useGlobalSearch } from './useGlobalSearch'

// Pressing this key anywhere outside a text field focuses search (`~/.claude/docs/ux.md`,
// Global searchbars). Ctrl+F stays with a page's own table search.
const SEARCH_SHORTCUT_KEY = '/'

// The topbar shows the closest few; View all opens the search page with every one.
const TOPBAR_RESULT_LIMIT = 5

// The two rows after the results. A result's id starts with its source's name, so neither
// can be mistaken for one.
const VIEW_ALL_KEY = 'search:viewAll'
const ASK_ELYSIA_KEY = 'search:askElysia'

// Phones leave the field narrow beside the topbar's buttons, so there the results span the
// screen just below the topbar; from `sm` they hang from the field.
const RESULTS_PLACEMENT = [
  'fixed inset-x-4 top-16 z-50',
  'sm:absolute sm:inset-x-auto sm:top-full sm:left-0 sm:mt-2 sm:w-full sm:min-w-[24rem]',
  'max-h-[min(32rem,calc(100dvh-5rem))] overflow-y-auto rounded-3xl bg-overlay p-1 shadow-overlay',
].join(' ')

// Elysium's one search, in the topbar: pages and actions registered beside each part of the
// app, and whatever further sources `useGlobalSearch` adds, the closest five first, then View
// all and Ask Elysia. Arrow keys move through the results while typing, Enter opens one, and
// each result that goes somewhere is a real link. Esc, choosing a result, or clicking
// anywhere else clears it.
export function GlobalSearch() {
  const { t } = useTranslation([ 'search', 'common' ])
  const [ query, setQuery ] = useState('')
  const [ isAsking, setIsAsking ] = useState(false)
  const containerRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const search = useGlobalSearch(query)
  const askElysia = useAskElysia()

  const trimmedQuery = query.trim()
  const isOpen = Boolean(trimmedQuery)
  const shownResults = search.results.slice(0, TOPBAR_RESULT_LIMIT)
  const sections = groupSearchResults(shownResults)

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== SEARCH_SHORTCUT_KEY || event.ctrlKey || event.metaKey || event.altKey) {
        return
      }

      // Typing a slash into any field must still type a slash, and a dialog keeps the keyboard.
      const target = event.target
      if (target instanceof HTMLElement && target.closest('input, textarea, [contenteditable="true"]')) {
        return
      }
      if (isModalOpen()) {
        return
      }

      event.preventDefault()
      inputRef.current?.focus()
    }

    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  useInteractOutside({
    ref: containerRef,
    isDisabled: !isOpen,
    onInteractOutside: () => setQuery(''),
  })

  function finish() {
    setQuery('')
    inputRef.current?.blur()
  }

  async function ask() {
    setIsAsking(true)
    const isSent = await askElysia(trimmedQuery)
    setIsAsking(false)
    // A question that failed to send stays typed, so it can be asked again.
    if (isSent) {
      finish()
    }
  }

  function onAction(key: Key) {
    if (key === ASK_ELYSIA_KEY) {
      void ask()
      return
    }

    const result = shownResults.find((candidate) => candidate.id === key)
    if (result?.target.kind === 'action') {
      result.target.run()
    }
    // A link, View all included, follows its own href.
    finish()
  }

  // Tabbing away clears it as clicking away does. Focus lost to nothing, such as a press on
  // the results' own padding or another window, leaves it as it is.
  function onBlur(event: FocusEvent<HTMLDivElement>) {
    const next = event.relatedTarget
    if (next instanceof Node && !event.currentTarget.contains(next)) {
      setQuery('')
    }
  }

  return <div ref={containerRef} className='relative w-full min-w-0 max-w-sm' onBlur={onBlur}>
    <Autocomplete inputValue={query} onInputChange={setQuery}>
      <SearchField
        aria-label={t('common:search.label')}
        className='w-full'
        onKeyDown={(event) => {
          // Esc with nothing typed leaves search rather than the page; with text, the field
          // clears itself first.
          if (event.key === 'Escape' && !query) {
            event.preventDefault()
            inputRef.current?.blur()
          }
        }}
      >
        <SearchField.Group className='bg-default shadow-none'>
          {search.isLoading
            ? <Spinner size='sm' className='ml-3' aria-label={t('results.loading')} />
            : <SearchField.SearchIcon />}
          {/* Shrinks with the field on a phone, so the clear button stays inside it. */}
          <SearchField.Input
            ref={inputRef}
            className='min-w-0'
            placeholder={t('common:search.placeholder')}
          />
          {query
            ? <SearchField.ClearButton />
            // Phones have no keyboard to press it on.
            : <Kbd className='mr-2 hidden md:inline-flex'>
              <Kbd.Content>{SEARCH_SHORTCUT_KEY}</Kbd.Content>
            </Kbd>}
        </SearchField.Group>
      </SearchField>

      {isOpen && <div className={RESULTS_PLACEMENT}>
        {search.isLoading && <p role='status' className='px-3 py-2 text-sm opacity-70'>{
          t('results.loading')
        }</p>}
        {!search.isLoading && !shownResults.length && <p role='status' className='px-3 py-2 text-sm opacity-70'>{
          t('results.none', { query: trimmedQuery })
        }</p>}
        <Menu aria-label={t('results.label')} onAction={onAction}>
          {sections.map((section) => <Menu.Section key={section.group}>
            <Header>{t(searchGroupLabelKeys[section.group])}</Header>
            {section.results.map((result) => <Menu.Item
              key={result.id}
              id={result.id}
              textValue={result.title}
              href={result.target.kind === 'link'
                ? result.target.href
                : undefined}
            >
              <result.icon className='size-4 shrink-0 opacity-80' aria-hidden />
              <div className='flex min-w-0 flex-col'>
                <Label className='truncate'>{result.title}</Label>
                {result.detail && <Description className='truncate'>{result.detail}</Description>}
              </div>
            </Menu.Item>)}
          </Menu.Section>)}
          {Boolean(shownResults.length) && <Separator />}
          {Boolean(shownResults.length) && <Menu.Item
            id={VIEW_ALL_KEY}
            textValue={t('results.viewAll')}
            href={getSearchUrl(trimmedQuery)}
          >
            <LuArrowRight className='size-4 shrink-0 opacity-80' aria-hidden />
            <Label>{t('results.viewAll')}</Label>
          </Menu.Item>}
          <Menu.Item
            id={ASK_ELYSIA_KEY}
            textValue={t('results.askElysia', { query: trimmedQuery })}
            isDisabled={isAsking}
          >
            {isAsking
              ? <Spinner size='sm' />
              : <LuSparkles className='size-4 shrink-0 opacity-80' aria-hidden />}
            <Label className='truncate'>{t('results.askElysia', { query: trimmedQuery })}</Label>
          </Menu.Item>
        </Menu>
      </div>}
    </Autocomplete>
  </div>
}
