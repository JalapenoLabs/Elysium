// Copyright © 2026 Jalapeno Labs

// Core
import { createContext, useContext } from 'react'

// The element in the page's titlebar that a list's table renders its search, filters,
// Columns, and export into (`~/.claude/docs/ux.md`, Table layout), through uikit's
// `controls={{ container }}`. `Page` provides it, so a table anywhere inside the page, such as
// one in a child route under a layout's tabs, reaches the titlebar the layout draws. Null
// until the titlebar mounts, and outside a `Page`.
export const PageToolsContext = createContext<Element | null>(null)

// The titlebar element for the page's one list. A table passes it on only where it is the
// page's list: the same table embedded beside other sections keeps its controls to itself.
export function usePageToolsContainer() {
  return useContext(PageToolsContext)
}
