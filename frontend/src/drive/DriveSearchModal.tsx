// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent } from 'react'
import type { StreamingSource } from '../components/streaming/StreamingSources'
import type { FileEntry, SearchFrame } from '../proto/generated/storage_pb'
import type { StatusKind } from '../theme/statusColors'

// Core
import { useEffect, useMemo, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Chip, Input, Modal, ModalContent } from '@heroui/react'
import { LuFile, LuFolder, LuSearch } from 'react-icons/lu'
import { StreamingSources } from '../components/streaming/StreamingSources'

// Misc
import { usePaletteEntries } from '../components/palette/registry'
import { openWorkspacePanel } from '../workspace/controller'
import { bindDriveSearchOpener } from './searchController'
import { revealInFiles } from './revealBus'
import { searchDrives } from './searchStream'
import { parentPath } from './paths'
import { SearchFrame_Status as searchStatus } from '../proto/generated/storage_pb'
import { UrlTree } from '../urls'

// How long the query rests before a search fires; typing aborts the previous
// stream and restarts.
const SEARCH_DEBOUNCE_MS = 400

// One Drive's live search state, patched frame by frame.
type SourceState = {
  driveId: string
  driveName: string
  status: SearchFrame_StatusValue
  error: string
  matches: FileEntry[]
}

type SearchFrame_StatusValue = SearchFrame['status']

const statusKindByFrameStatus = {
  [searchStatus.UNSPECIFIED]: 'idle',
  [searchStatus.SEARCHING]: 'active',
  [searchStatus.SLOW]: 'active',
  [searchStatus.DONE]: 'done',
  [searchStatus.ERROR]: 'error',
} as const satisfies Record<SearchFrame_StatusValue, StatusKind>

// The unified search (docs/storage.md): live, federated, streaming, index-free.
// Results render progressively as frames arrive, each Drive reports its own
// status in the grammar, and closing (or retyping) aborts the stream
// server-side via the fetch AbortSignal. Selecting a result reveals it in the
// Files workspace panel.
export function DriveSearchModal() {
  const { t } = useTranslation()
  const navigate = useNavigate()

  const [ isOpen, setIsOpen ] = useState(false)
  const [ query, setQuery ] = useState('')
  const [ sourcesById, setSourcesById ] = useState<Map<string, SourceState>>(new Map())
  const [ activeIndex, setActiveIndex ] = useState(0)
  const [ hasStreamFailed, setHasStreamFailed ] = useState(false)

  const abortRef = useRef<AbortController | null>(null)

  function stopSearch() {
    abortRef.current?.abort()
    abortRef.current = null
  }

  // The taskbar search (and anything else) opens this modal through the
  // controller seam.
  useEffect(() => {
    return bindDriveSearchOpener((initialQuery) => {
      setQuery(initialQuery)
      setSourcesById(new Map())
      setActiveIndex(0)
      setIsOpen(true)
    })
  }, [])

  // The palette knows the search too, keyboard-first.
  const paletteEntries = useMemo(() => [
    {
      id: 'drive.search',
      label: t('drive.search.paletteLabel'),
      keywords: t('drive.search.paletteKeywords'),
      section: t('palette.sections.navigate'),
      run: () => {
        setQuery('')
        setSourcesById(new Map())
        setActiveIndex(0)
        setIsOpen(true)
      },
    },
  ], [ t ])
  usePaletteEntries('drive.search', paletteEntries)

  // Debounced streaming search; every keystroke aborts the previous stream.
  useEffect(() => {
    if (!isOpen) {
      stopSearch()
      return undefined
    }

    const trimmedQuery = query.trim()
    if (!trimmedQuery) {
      setSourcesById(new Map())
      return undefined
    }

    const timer = setTimeout(() => {
      stopSearch()
      const controller = new AbortController()
      abortRef.current = controller
      setSourcesById(new Map())
      setActiveIndex(0)
      setHasStreamFailed(false)

      searchDrives({
        query: trimmedQuery,
        driveIds: [],
        signal: controller.signal,
        onFrame: (frame) => {
          setSourcesById((previous) => {
            const next = new Map(previous)
            const existing = next.get(frame.driveId)
            next.set(frame.driveId, {
              driveId: frame.driveId,
              driveName: frame.driveName,
              status: frame.status,
              error: frame.error,
              // DONE carries the final set; SEARCHING batches accumulate.
              matches: frame.status === searchStatus.DONE
                ? frame.matches
                : [ ...(existing?.matches ?? []), ...frame.matches ],
            })
            return next
          })
        },
      }).catch((error) => {
        if (!controller.signal.aborted) {
          console.debug('drive search stream failed', { error })
          setHasStreamFailed(true)
        }
      })
    }, SEARCH_DEBOUNCE_MS)

    return () => clearTimeout(timer)
  }, [ isOpen, query ])

  function close() {
    stopSearch()
    setIsOpen(false)
  }

  const sources = [ ...sourcesById.values() ]
  const strip: StreamingSource[] = sources.map((source) => ({
    id: source.driveId,
    label: source.driveName,
    status: statusKindByFrameStatus[source.status],
    detail: source.status === searchStatus.ERROR
      ? source.error
      : source.status === searchStatus.SLOW
        ? t('drive.search.slow')
        : t('drive.search.matchCount', { count: source.matches.length }),
  }))

  // Flat result list in source order; each row remembers its Drive.
  const results = sources.flatMap((source) =>
    source.matches.map((entry) => ({ source, entry })))

  function openResult(result: { source: SourceState, entry: FileEntry } | undefined) {
    if (!result) {
      console.debug('no search result at the active index, ignoring')
      return
    }
    close()
    navigate(UrlTree.home)
    openWorkspacePanel('files')
    revealInFiles({
      driveId: result.source.driveId,
      path: result.entry.isDir ? result.entry.path : parentPath(result.entry.path),
    })
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActiveIndex((index) => Math.min(index + 1, results.length - 1))
    }
    else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActiveIndex((index) => Math.max(index - 1, 0))
    }
    else if (event.key === 'Enter') {
      event.preventDefault()
      openResult(results[activeIndex])
    }
    else if (event.key === 'Escape') {
      // The input owns focus while open, so it handles Escape itself (the
      // modal's own dismissal path is swallowed by the focused field).
      event.preventDefault()
      close()
    }
  }

  return <Modal
    isOpen={isOpen}
    onClose={close}
    placement='top'
    backdrop='blur'
    size='2xl'
    hideCloseButton
    className='mt-[10vh]'
  >
    <ModalContent>
      <div className='p-2'>
        <Input
          autoFocus
          size='lg'
          variant='flat'
          aria-label={t('drive.search.inputLabel')}
          placeholder={t('drive.search.placeholder')}
          startContent={<LuSearch className='h-5 w-5 opacity-50' />}
          value={query}
          onValueChange={(value) => {
            setQuery(value)
            setActiveIndex(0)
          }}
          onKeyDown={onKeyDown}
        />

        { Boolean(strip.length) && (
          <StreamingSources sources={strip} className='px-3 py-2' />
        ) }

        <div className='mt-1 max-h-[50vh] overflow-y-auto' role='listbox' aria-label={t('drive.search.resultsLabel')}>
          { results.map((result, index) => (
            <button
              key={`${result.source.driveId}:${result.entry.path}`}
              type='button'
              role='option'
              aria-selected={index === activeIndex}
              className={
                'flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors '
                + (index === activeIndex ? 'bg-primary/15' : 'hover:bg-default/40')
              }
              onMouseEnter={() => setActiveIndex(index)}
              onClick={() => openResult(result)}
            >
              { result.entry.isDir
                ? <LuFolder className='h-4 w-4 shrink-0 opacity-70' />
                : <LuFile className='h-4 w-4 shrink-0 opacity-50' /> }
              <span className='min-w-0 flex-1'>
                <span className='block truncate'>{ result.entry.name }</span>
                <span className='block truncate text-xs opacity-50'>{ result.entry.path }</span>
              </span>
              <Chip size='sm' variant='flat'>{ result.source.driveName }</Chip>
            </button>
          )) }
          { hasStreamFailed && (
            <p className='px-3 py-6 text-center text-sm text-status-error'>{
              t('drive.search.failed')
            }</p>
          ) }
          { !hasStreamFailed && !results.length && query.trim() && (
            <p className='px-3 py-6 text-center text-sm opacity-50'>{
              t('drive.search.noResults')
            }</p>
          ) }
        </div>
      </div>
    </ModalContent>
  </Modal>
}
