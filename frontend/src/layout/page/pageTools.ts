// Copyright © 2026 Jalapeno Labs

// Core
import { createContext, useContext, useLayoutEffect } from 'react'

export type PageTools = {
  // The element in the page's titlebar that a list's table renders its search, filters,
  // Columns, and export into. Null until the titlebar holds it.
  container: Element | null
  // Tells the page a list renders its controls in the titlebar, so the page draws the slot
  // for them. Returns the release, for when the list goes.
  claim: () => () => void
}

// The page's titlebar slot for its one list (`~/.claude/docs/ux.md`, Table layout), filled
// through uikit's `controls={{ container }}`. `Page` provides it, so a table anywhere inside
// the page, such as one in a child route under a layout's tabs, reaches the titlebar the
// layout draws. Null outside a `Page`.
export const PageToolsContext = createContext<PageTools | null>(null)

// The titlebar element for the page's one list. Only the page's own list calls it: calling it
// claims the slot, and a table embedded beside other sections keeps its controls to itself.
// The claim is what makes the titlebar draw the slot: the controls arrive through a portal the
// page never sees, and the header decides from React state alone whether its right side shows,
// never from what CSS can find inside it, which Firefox does not re-check as content arrives.
export function usePageToolsContainer() {
  const tools = useContext(PageToolsContext)
  const claim = tools?.claim

  // Before paint, so the slot is there by the time the list's first frame shows.
  useLayoutEffect(() => claim?.(), [ claim ])

  return tools?.container ?? null
}
