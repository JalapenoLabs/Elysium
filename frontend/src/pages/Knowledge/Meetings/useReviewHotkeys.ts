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

// Where Space presses what has focus, such as a button or a switch, and so plays nothing.
const PRESSES_SPACE_SELECTOR = [
  'button',
  'a[href]',
  '[role="button"]',
  '[role="checkbox"]',
  '[role="switch"]',
  '[role="radio"]',
  '[role="tab"]',
  '[role="option"]',
  '[role="menuitem"]',
].join(', ')

// Where the arrows move through what has focus, such as tabs or the pills of a radio group.
const MOVES_ON_ARROWS_SELECTOR = [
  '[role="slider"]',
  '[role="radiogroup"]',
  '[role="tablist"]',
  '[role="tree"]',
  '[role="treegrid"]',
  '[role="grid"]',
].join(', ')

const jumpStepByKey: Partial<Record<string, 1 | -1>> = {
  ArrowLeft: -1,
  ArrowRight: 1,
}

type Handlers = {
  // Each answers whether it used the key, so one it did not use keeps its usual meaning.
  onEscape: () => boolean
  onEdit: () => boolean
  onCycleMark: (step: 1 | -1) => boolean
  // Held down, Space repeats; a repeat is used without playing, so the page neither scrolls nor
  // flicks between playing and paused.
  onPlayPause: (isRepeat: boolean) => boolean
  onJump: (step: 1 | -1) => boolean
  onUndo: () => void
  // What Ctrl+C copies: the words selected, or null to leave copying to the browser.
  selectedText: () => string | null
}

function isWithin(target: EventTarget | null, selector: string) {
  return target instanceof Element && Boolean(target.closest(selector))
}

function ownsKeys(target: EventTarget | null) {
  return isWithin(target, OWNS_KEYS_SELECTOR)
}

// The keys of a meeting's page (docs/frontend.md, Hotkeys), outside a text field and while no
// dialog or menu is open: Esc clears what is picked, E edits the words selected, Tab moves between
// the marks under the word clicked, Space plays and pauses, the left and right arrows jump back
// and forward, Ctrl+Z (Cmd+Z) undoes the page's last change, and Ctrl+C copies the words selected.
// They listen as the key travels down, so Esc is used here before the page's own Esc goes up a
// level, which a used key never does (`readPageShortcut`), and Space and the arrows before the
// player's own controls take them, so they mean the same wherever focus is.
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
    if (event.key === 'Tab' && handlers.onCycleMark(event.shiftKey ? -1 : 1)) {
      event.preventDefault()
      return
    }
    if (event.key.toLowerCase() === 'e' && !event.shiftKey && !event.repeat && handlers.onEdit()) {
      event.preventDefault()
      return
    }
    if (event.key === ' ' && !event.shiftKey && !isWithin(event.target, PRESSES_SPACE_SELECTOR)) {
      if (handlers.onPlayPause(event.repeat)) {
        event.preventDefault()
      }
      return
    }
    const step = jumpStepByKey[event.key]
    if (step && !event.shiftKey && !isWithin(event.target, MOVES_ON_ARROWS_SELECTOR) && handlers.onJump(step)) {
      event.preventDefault()
    }
  })

  const onCopy = useEffectEvent((event: ClipboardEvent) => {
    if (ownsKeys(event.target) || !event.clipboardData) {
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
