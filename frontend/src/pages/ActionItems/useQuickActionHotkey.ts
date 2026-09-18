// Copyright © 2026 Jalapeno Labs

// Core
import { useHotkey } from '@jalapenolabs/uikit'

// A single-letter shortcut for a quick action on Next and in the inbox. uikit already skips
// keys typed into a text field; this also skips them while a dialog, menu, or list box has
// focus (a snooze date's segments are not text fields), and when a modifier is held, so
// Ctrl+R still reloads rather than resolving the item.
export function useQuickActionHotkey(key: string, action: () => void, isEnabled: boolean) {
  useHotkey([ key ], (event) => {
    if (!isEnabled || event.ctrlKey || event.metaKey || event.altKey) {
      return
    }
    const target = event.target
    if (target instanceof Element && target.closest('[role="dialog"], [role="menu"], [role="listbox"]')) {
      return
    }
    event.preventDefault()
    action()
  })
}
