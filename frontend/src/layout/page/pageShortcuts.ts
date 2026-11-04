// Copyright © 2026 Jalapeno Labs

// What a key press asks of a page: Esc goes back to its parent, and Ctrl+S (Cmd+S
// on a Mac) presses Save.
export type PageShortcut = 'leave' | 'save'

const MODAL_SELECTOR = '[role="dialog"], [role="alertdialog"]'

// Where a key belongs to something open over the page rather than to the page. React Aria
// closes its own overlays on Esc, and a list box or menu handles its own keys.
const OVERLAY_SELECTOR = `${MODAL_SELECTOR}, [role="menu"], [role="listbox"]`

export function readPageShortcut(event: KeyboardEvent): PageShortcut | null {
  const isSave = (event.ctrlKey || event.metaKey)
    && !event.altKey
    && !event.shiftKey
    && event.key.toLowerCase() === 's'
  if (isSave) {
    return 'save'
  }

  const isPlainEscape = event.key === 'Escape'
    && !event.ctrlKey
    && !event.metaKey
    && !event.altKey
    && !event.shiftKey
  // A component that used the key, such as a search field clearing itself, marks it.
  if (!isPlainEscape || event.defaultPrevented || event.repeat) {
    return null
  }
  const target = event.target
  if (target instanceof Element && target.closest(OVERLAY_SELECTOR)) {
    return null
  }
  if (isModalOpen()) {
    return null
  }
  return 'leave'
}

// A modal owns the keyboard while it is open, even when focus sits outside it, such as in
// the moment after it opens.
export function isModalOpen() {
  return Boolean(document.querySelector(MODAL_SELECTOR))
}
