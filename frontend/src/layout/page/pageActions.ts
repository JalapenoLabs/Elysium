// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'

type PageActionBase = {
  // Unique among the page's actions; also the menu item's key.
  id: string
  label: string
  // Leads the label, in the menu and on a lone action's button.
  icon: IconType
  isDisabled?: boolean
  // Shows the action working: on its own button, or on the Actions trigger.
  isPending?: boolean
  // Destructive, such as Delete: red, at the bottom of the menu below a divider, and never
  // a bare button in the titlebar.
  isDanger?: boolean
}

// One thing a page's titlebar offers to do. It acts, goes somewhere (a real link, so it can
// be opened in a new tab), or opens a few choices of its own as a submenu.
export type PageAction = PageActionBase & (
  | { onAction: () => void }
  | { href: string }
  | { items: PageAction[] }
)

export type ArrangedPageActions = {
  // A lone, harmless action stands as its own button.
  button: PageAction | null
  // Anything else is the Actions menu: the harmless ones, then the destructive ones below a
  // divider. Both are empty when there is a button or nothing at all.
  regular: PageAction[]
  dangerous: PageAction[]
}

// Applies the titlebar's rule (`~/.claude/docs/ux.md`, Standard page layout): more than one
// action nests into one Actions menu, kept on the far right, with destructive items last.
export function arrangePageActions(actions: PageAction[]): ArrangedPageActions {
  const regular: PageAction[] = []
  const dangerous: PageAction[] = []
  for (const action of actions) {
    if (action.isDanger) {
      dangerous.push(action)
      continue
    }
    regular.push(action)
  }

  if (actions.length === 1 && regular.length === 1) {
    return { button: regular[0], regular: [], dangerous: []}
  }
  return { button: null, regular, dangerous }
}

// Whether anything in the menu is working, so its trigger can say so.
export function isAnyPageActionPending(actions: PageAction[]): boolean {
  for (const action of actions) {
    if (action.isPending) {
      return true
    }
    if ('items' in action && isAnyPageActionPending(action.items)) {
      return true
    }
  }
  return false
}
