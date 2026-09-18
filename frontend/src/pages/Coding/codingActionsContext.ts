// Copyright © 2026 Jalapeno Labs

import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { createContext, useContext } from 'react'

// Page-level actions for components inside Dockview panels. Dockview renders panels
// through portals, so they share the page's React tree but not its props: the page
// owns the layout and the dialogs, and panels reach them through this context.
export type CodingActions = {
  openSession: (session: CodingSession) => void
  // Opens New session, started from an action item when one is given.
  createSession: (actionItemId?: string) => void
  renameSession: (session: CodingSession) => void
  deleteSession: (session: CodingSession) => void
}

export const CodingActionsContext = createContext<CodingActions | null>(null)

export function useCodingActions() {
  const actions = useContext(CodingActionsContext)
  if (!actions) {
    throw new Error('useCodingActions must be used inside the Coding page')
  }
  return actions
}
