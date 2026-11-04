// Copyright © 2026 Jalapeno Labs

import type { RefCallback } from 'react'

// Core
import { useCallback, useEffect, useRef } from 'react'

// Misc
import { isModalOpen } from '../layout/page/pageShortcuts'
import { useInitialFocus } from './useInitialFocus'

// Where typing belongs to a field already, so Ctrl+F keeps the browser's find there.
const TEXT_ENTRY_SELECTOR = 'input, textarea, select, [contenteditable="true"]'

// Whether a key press is Ctrl+F (Cmd+F on a Mac) that a page's search may take: not typed in a
// field, not already taken by another search, and not while a modal owns the keyboard.
export function readsAsFind(event: KeyboardEvent) {
  const isFind = (event.ctrlKey || event.metaKey)
    && !event.altKey
    && !event.shiftKey
    && event.key.toLowerCase() === 'f'
  if (!isFind || event.defaultPrevented) {
    return false
  }
  const target = event.target
  if (target instanceof Element && target.closest(TEXT_ENTRY_SELECTOR)) {
    return false
  }
  return !isModalOpen()
}

// Makes the input it is given the page's search: Ctrl+F (Cmd+F) focuses it and selects what it
// holds instead of opening the browser's find, unless focus is already in a text field
// (`~/.claude/docs/ux.md`, Hotkeys). uikit's table searches do the same through their
// `focusHotkey`, so a page has one search that answers, the first to take the key; the topbar's
// global search answers `/` instead. Pass `shouldFocusOnOpen` for a page that is its search,
// so typing starts there as it opens.
export function useFindHotkey(shouldFocusOnOpen = false): RefCallback<HTMLInputElement> {
  const inputRef = useRef<HTMLInputElement | null>(null)
  const focusOnOpen = useInitialFocus(shouldFocusOnOpen)

  useEffect(() => {
    function focusOnFind(event: KeyboardEvent) {
      const input = inputRef.current
      if (!input || !readsAsFind(event)) {
        return
      }
      event.preventDefault()
      input.focus()
      input.select()
    }

    window.addEventListener('keydown', focusOnFind)
    return () => window.removeEventListener('keydown', focusOnFind)
  }, [])

  return useCallback((input: HTMLInputElement | null) => {
    inputRef.current = input
    focusOnOpen(input)
  }, [ focusOnOpen ])
}
