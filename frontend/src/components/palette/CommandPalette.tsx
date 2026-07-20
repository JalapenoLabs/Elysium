// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent } from 'react'
import type { RankedPaletteEntry } from './registry'

// Core
import { useMemo, useState, useSyncExternalStore } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Input, Modal, ModalContent } from '@heroui/react'
import { FiSearch } from 'react-icons/fi'

// Utility
import { useHotkey } from '@jalapenolabs/uikit'

// Misc
import { listPaletteEntries, rankPaletteEntries, subscribePalette } from './registry'

// The universal ctrl/cmd+P navigator (docs/ux.md, keyboard-first): one modal,
// every registered destination and action, fuzzy-ranked as you type. Arrow
// keys move, Enter runs, Escape leaves. Pillars contribute entries through the
// palette registry; the palette itself knows none of them.
export function CommandPalette() {
  const { t } = useTranslation()
  const [ isOpen, setIsOpen ] = useState(false)
  const [ query, setQuery ] = useState('')
  const [ activeIndex, setActiveIndex ] = useState(0)

  const entries = useSyncExternalStore(subscribePalette, listPaletteEntries)

  const ranked = useMemo(
    () => rankPaletteEntries(query, entries),
    [ query, entries ],
  )

  // Opening resets the query so a keystroke that slipped in during the close
  // animation (the input stays mounted while fading out) can never carry a
  // stale search into the next open.
  function open() {
    setQuery('')
    setActiveIndex(0)
    setIsOpen(true)
  }

  // Both chords open the palette; browsers reserve ctrl+P for print, so the
  // hotkey swallows the default.
  useHotkey([ 'Control', 'p' ], open, { preventDefault: true })
  useHotkey([ 'Meta', 'p' ], open, { preventDefault: true })

  function close() {
    setIsOpen(false)
  }

  function runEntry(ranked: RankedPaletteEntry | undefined) {
    if (!ranked) {
      console.debug('CommandPalette has no entry at the active index, ignoring Enter')
      return
    }
    close()
    ranked.entry.run()
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActiveIndex((index) => Math.min(index + 1, ranked.length - 1))
    }
    else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActiveIndex((index) => Math.max(index - 1, 0))
    }
    else if (event.key === 'Enter') {
      event.preventDefault()
      runEntry(ranked[activeIndex])
    }
    else if (event.key === 'Escape') {
      // The input owns focus while the palette is open, so it handles Escape
      // itself rather than relying on the modal's dismissal path (which the
      // focused field swallows).
      event.preventDefault()
      close()
    }
  }

  return <Modal
    isOpen={isOpen}
    onClose={close}
    placement='top'
    backdrop='blur'
    hideCloseButton
    className='mt-[10vh]'
  >
    <ModalContent>
      <div className='p-2'>
        <Input
          autoFocus
          size='lg'
          variant='flat'
          aria-label={t('palette.inputLabel')}
          placeholder={t('palette.placeholder')}
          startContent={<FiSearch className='h-5 w-5 opacity-50' />}
          value={query}
          onValueChange={(value) => {
            setQuery(value)
            setActiveIndex(0)
          }}
          onKeyDown={onKeyDown}
        />
        <div className='mt-2 max-h-[50vh] overflow-y-auto' role='listbox' aria-label={t('palette.resultsLabel')}>
          {ranked.length
            ? ranked.map((result, index) => (
              <button
                key={result.entry.id}
                type='button'
                role='option'
                aria-selected={index === activeIndex}
                className={
                  'flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-sm transition-colors '
                  + (index === activeIndex ? 'bg-primary/15' : 'hover:bg-default/40')
                }
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => runEntry(result)}
              >
                {result.entry.icon && (
                  <span aria-hidden className='opacity-60'>{
                    result.entry.icon
                  }</span>
                )}
                <span className='min-w-0 flex-1 truncate'>
                  <MarkedLabel
                    label={result.entry.label}
                    indices={result.labelIndices}
                  />
                </span>
                {result.entry.section && (
                  <span className='text-xs uppercase tracking-widest opacity-40'>{
                    result.entry.section
                  }</span>
                )}
              </button>
            ))
            : <p className='px-3 py-6 text-center text-sm opacity-50'>{
              t('palette.noResults')
            }</p>}
        </div>
      </div>
    </ModalContent>
  </Modal>
}

type MarkedLabelProps = {
  label: string
  indices: number[]
}

// Renders the fuzzy-matched characters emphasized. uikit's HighlightFuzzy is
// substring-only, while these indices come from subsequence matching.
function MarkedLabel(props: MarkedLabelProps) {
  if (!props.indices.length) {
    return <>{props.label}</>
  }

  const matched = new Set(props.indices)
  return <>{
    [ ...props.label ].map((character, index) => (
      matched.has(index)
        ? <mark key={index} className='bg-transparent font-semibold text-primary'>{character}</mark>
        : <span key={index}>{character}</span>
    ))
  }</>
}
