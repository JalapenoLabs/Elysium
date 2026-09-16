// Copyright © 2026 Jalapeno Labs

import type { DockviewApi, DockviewTheme } from 'dockview-react'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Misc
import { CODING_LAYOUT_STORAGE_KEY } from '../../constants'

// Panel component names, as registered with DockviewReact.
export const CODING_PANEL_COMPONENTS = {
  sessions: 'sessions',
  conversation: 'conversation',
} as const

export const SESSIONS_PANEL_ID = 'sessions'

const CONVERSATION_PANEL_PREFIX = 'session:'

export type ConversationPanelParams = {
  sessionId: number
}

// Colors come from `src/theme/dockview.css`, which points Dockview's variables at the
// Matter tokens, so the layout follows light and dark mode with the rest of the app.
export const elysiumDockviewTheme: DockviewTheme = {
  name: 'elysium',
  className: 'dockview-theme-elysium',
}

export function getConversationPanelId(sessionId: number) {
  return `${CONVERSATION_PANEL_PREFIX}${sessionId}`
}

export function showSessionsPanel(api: DockviewApi, title: string) {
  const existing = api.getPanel(SESSIONS_PANEL_ID)
  if (existing) {
    existing.api.setActive()
    return
  }

  api.addPanel({
    id: SESSIONS_PANEL_ID,
    component: CODING_PANEL_COMPONENTS.sessions,
    title,
    position: { direction: 'left' },
  })
}

// Focuses a session's conversation, opening it beside the sessions list the first
// time and as a tab among other open conversations after that.
export function openConversation(api: DockviewApi, session: CodingSession) {
  const panelId = getConversationPanelId(session.id)
  const existing = api.getPanel(panelId)
  if (existing) {
    existing.api.setActive()
    return
  }

  const openConversationPanel = api.panels.find((panel) => panel.id.startsWith(CONVERSATION_PANEL_PREFIX))
  const params: ConversationPanelParams = { sessionId: session.id }

  if (openConversationPanel) {
    api.addPanel({
      id: panelId,
      component: CODING_PANEL_COMPONENTS.conversation,
      title: session.title,
      params,
      position: { referencePanel: openConversationPanel.id, direction: 'within' },
    })
    return
  }

  if (api.getPanel(SESSIONS_PANEL_ID)) {
    api.addPanel({
      id: panelId,
      component: CODING_PANEL_COMPONENTS.conversation,
      title: session.title,
      params,
      position: { referencePanel: SESSIONS_PANEL_ID, direction: 'right' },
    })
    return
  }

  api.addPanel({
    id: panelId,
    component: CODING_PANEL_COMPONENTS.conversation,
    title: session.title,
    params,
  })
}

// Restores the saved layout, returning false when there is none or it no longer fits
// this build's panels.
export function restoreLayout(api: DockviewApi) {
  let saved: string | null = null
  try {
    saved = window.localStorage.getItem(CODING_LAYOUT_STORAGE_KEY)
  }
  catch (error) {
    console.debug('The Coding layout could not be read from localStorage', { error })
  }
  if (!saved) {
    return false
  }

  try {
    api.fromJSON(JSON.parse(saved))
    return true
  }
  catch (error) {
    console.debug('Discarding a saved Coding layout that no longer loads', { error })
    api.clear()
    return false
  }
}

export function saveLayout(api: DockviewApi) {
  try {
    window.localStorage.setItem(CODING_LAYOUT_STORAGE_KEY, JSON.stringify(api.toJSON()))
  }
  catch (error) {
    console.debug('The Coding layout could not be saved to localStorage', { error })
  }
}

export function forgetLayout() {
  try {
    window.localStorage.removeItem(CODING_LAYOUT_STORAGE_KEY)
  }
  catch (error) {
    console.debug('The Coding layout could not be removed from localStorage', { error })
  }
}
