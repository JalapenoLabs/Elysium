// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useEffectEvent } from 'react'

// Misc
import { isModalOpen } from '../../../layout/page/pageShortcuts'

// Where a key belongs to what has focus: typing in a field, or a menu or list moving through its
// own items.
const OWNS_KEYS_SELECTOR = [
  'input',
  'textarea',
  'select',
  '[contenteditable="true"]',
  '[role="menu"]',
  '[role="listbox"]',
  '[role="dialog"]',
  '[role="alertdialog"]',
].join(', ')

type Handlers = {
  // Each answers whether it used the key, so one it did not use keeps its usual meaning.
  onEscape: () => boolean
  onEdit: () => boolean
  onCycleMark: (step: 1 | -1) => boolean
  onUndo: () => void
  // What Ctrl+C copies: the words selected, or null to leave copying to the browser.
  selectedText: () => string | null
}

function ownsKeys(target: EventTarget | null) {
  return target instanceof Element && Boolean(target.closest(OWNS_KEYS_SELECTOR))
}

// Where the transcript's own keys apply: the transcript and its floating bar, which carry
// `data-review-keys`. A press on a word gives the transcript focus (`FinalTranscript`), so a key
// pressed after it lands there. Anywhere else (a button in the lane, the topbar, the page before
// any word was pressed) Tab moves focus, E is a letter, and Ctrl+C copies, as everywhere.
function isOnTranscript(target: EventTarget | null) {
  return target instanceof Element && Boolean(target.closest('[data-review-keys]'))
}

// The keys of a meeting's page (docs/frontend.md, Hotkeys), outside a text field and while no
// dialog or menu is open: Esc clears what is picked and Ctrl+Z (Cmd+Z) undoes the page's last
// change; on the transcript alone (`isOnTranscript`), E edits the words selected, Tab moves between
// the marks under the word clicked, and Ctrl+C copies the words selected. They listen as the key
// travels down, so Esc is used here before the page's own Esc goes up a level, which a used key
// never does (`readPageShortcut`).
export function useReviewHotkeys(handlers: Handlers) {
  const onKeyDown = useEffectEvent((event: KeyboardEvent) => {
    if (event.defaultPrevented || event.altKey || ownsKeys(event.target) || isModalOpen()) {
      return
    }
    const hasCommand = event.ctrlKey || event.metaKey

    if (hasCommand && !event.shiftKey && event.key.toLowerCase() === 'z') {
      event.preventDefault()
      handlers.onUndo()
      return
    }
    if (hasCommand) {
      return
    }
    if (event.key === 'Escape' && !event.shiftKey && handlers.onEscape()) {
      event.preventDefault()
      return
    }
    if (!isOnTranscript(event.target)) {
      return
    }
    if (event.key === 'Tab' && handlers.onCycleMark(event.shiftKey ? -1 : 1)) {
      event.preventDefault()
      return
    }
    if (event.key.toLowerCase() === 'e' && !event.shiftKey && !event.repeat && handlers.onEdit()) {
      event.preventDefault()
    }
  })

  const onCopy = useEffectEvent((event: ClipboardEvent) => {
    // A copy is aimed at the selection, which the transcript never holds (it draws its own), so
    // whose copy it is is read from focus: one meant for text in the lane copies that text.
    const focused = document.activeElement
    if (ownsKeys(focused) || !isOnTranscript(focused) || !event.clipboardData) {
      return
    }
    const text = handlers.selectedText()
    if (text === null) {
      return
    }
    event.preventDefault()
    event.clipboardData.setData('text/plain', text)
  })

  useEffect(() => {
    const keyDown = (event: KeyboardEvent) => onKeyDown(event)
    const copy = (event: ClipboardEvent) => onCopy(event)
    window.addEventListener('keydown', keyDown, true)
    document.addEventListener('copy', copy)
    return () => {
      window.removeEventListener('keydown', keyDown, true)
      document.removeEventListener('copy', copy)
    }
  }, [])
}
