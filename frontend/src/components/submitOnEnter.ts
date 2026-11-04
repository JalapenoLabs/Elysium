// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent as ReactKeyboardEvent } from 'react'

// Where Enter belongs to the control it is pressed on rather than to the form: a button or
// link presses itself, and a list box, menu, calendar grid, or open combobox picks what is
// highlighted.
const OWN_ENTER_SELECTOR = [
  'button',
  'a[href]',
  '[role="button"]',
  '[role="link"]',
  '[role="listbox"]',
  '[role="menu"]',
  '[role="grid"]',
  '[aria-expanded="true"]',
].join(', ')

// Whether a key press submits the form it is pressed in (`~/.claude/docs/ux.md`, Hotkeys):
// Enter anywhere in it, except in a text area, where Enter adds a line and Ctrl or Cmd with
// Enter submits instead. Ctrl or Cmd with Enter submits from anywhere. Enter is left to a
// control that uses it itself, to a component that already handled it, and to a keyboard
// still composing a character. Never use it for a destructive dialog: Enter never confirms one.
export function readsAsSubmit(event: KeyboardEvent) {
  if (event.key !== 'Enter' || event.altKey || event.shiftKey || event.repeat) {
    return false
  }
  if (event.isComposing || event.defaultPrevented) {
    return false
  }
  const target = event.target
  if (!(target instanceof Element)) {
    return false
  }
  const hasModifier = event.ctrlKey || event.metaKey
  if (target instanceof HTMLTextAreaElement) {
    return hasModifier
  }
  return !target.closest(OWN_ENTER_SELECTOR)
}

// A form's `onKeyDownCapture` that submits it as `readsAsSubmit` describes, through its own
// `onSubmit`, so pressing Enter and pressing the submit button can never disagree. It listens
// as the key travels down rather than up: React Aria's date and time fields take Enter on its
// way up (their group treats it as a press) and stop it there, so a dialog whose field is a
// date would never hear it. Controls that use Enter themselves are left alone by what they
// are (`OWN_ENTER_SELECTOR`), since none of them has handled it yet. A form submitted this way
// skips the browser's check of a disabled submit button, so its `onSubmit` checks for itself
// that it can go ahead.
export function submitOnEnter(event: ReactKeyboardEvent<HTMLFormElement>) {
  if (!readsAsSubmit(event.nativeEvent)) {
    return
  }
  // Instead of the browser's own submit from a text input, so the form submits once.
  event.preventDefault()
  event.currentTarget.requestSubmit()
}
