// Copyright © 2026 Jalapeno Labs

import type { DockviewApi, DockviewReadyEvent } from 'dockview-react'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'
import type { CodingActions } from './codingActionsContext'

// Core
import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, useOverlayState } from '@heroui/react'
import { DockviewReact } from 'dockview-react'
import { LuLayoutPanelLeft, LuPlus, LuRotateCcw } from 'react-icons/lu'
import { ConversationPanel } from './ConversationPanel'
import { CreateSessionModal } from './CreateSessionModal'
import { DeleteSessionDialog } from './DeleteSessionDialog'
import { RenameSessionModal } from './RenameSessionModal'
import { SessionsPanel } from './SessionsPanel'

// Misc
import { CodingActionsContext } from './codingActionsContext'
import {
  CODING_PANEL_COMPONENTS,
  elysiumDockviewTheme,
  forgetLayout,
  openConversation,
  restoreLayout,
  saveLayout,
  showSessionsPanel,
} from './codingLayout'

const panelComponents = {
  [CODING_PANEL_COMPONENTS.sessions]: SessionsPanel,
  [CODING_PANEL_COMPONENTS.conversation]: ConversationPanel,
}

// The Coding area: a Dockview workspace holding the sessions overview and one
// conversation panel per open session, arranged however the user drags them. The
// layout persists per browser. This page owns the dialogs; panels open them through
// CodingActionsContext.
export function CodingPage() {
  const { t } = useTranslation('coding')
  const dockviewApiRef = useRef<DockviewApi | null>(null)

  const createState = useOverlayState()
  const renameState = useOverlayState()
  const deleteState = useOverlayState()
  const [ selectedSession, setSelectedSession ] = useState<CodingSession | null>(null)
  // Remounting a form per opening resets it.
  const [ formSession, setFormSession ] = useState(0)

  const actions = useMemo<CodingActions>(() => ({
    openSession: (session) => {
      if (!dockviewApiRef.current) {
        console.debug('CodingPage cannot open a session before Dockview is ready', { sessionId: session.id })
        return
      }
      openConversation(dockviewApiRef.current, session)
    },
    createSession: () => {
      setFormSession((session) => session + 1)
      createState.open()
    },
    renameSession: (session) => {
      setSelectedSession(session)
      setFormSession((formKey) => formKey + 1)
      renameState.open()
    },
    deleteSession: (session) => {
      setSelectedSession(session)
      deleteState.open()
    },
  }), [ createState, renameState, deleteState ])

  function onReady(event: DockviewReadyEvent) {
    dockviewApiRef.current = event.api

    if (!restoreLayout(event.api)) {
      showSessionsPanel(event.api, t('panels.sessions'))
    }
    event.api.onDidLayoutChange(() => saveLayout(event.api))
  }

  function resetLayout() {
    const api = dockviewApiRef.current
    if (!api) {
      console.debug('CodingPage cannot reset the layout before Dockview is ready')
      return
    }
    forgetLayout()
    api.clear()
    showSessionsPanel(api, t('panels.sessions'))
  }

  return <CodingActionsContext.Provider value={actions}>
    <div className='flex h-full flex-col'>
      <div className='level shrink-0 border-b border-separator px-6 py-2'>
        <h1 className='text-lg font-semibold'>{
          t('title')
        }</h1>
        <div className='level-right gap-2'>
          <Button
            size='sm'
            variant='ghost'
            onPress={() => {
              if (dockviewApiRef.current) {
                showSessionsPanel(dockviewApiRef.current, t('panels.sessions'))
              }
            }}
          >
            <LuLayoutPanelLeft className='size-4' aria-hidden />
            <span>{t('panels.sessions')}</span>
          </Button>
          <Button
            size='sm'
            variant='ghost'
            onPress={resetLayout}
          >
            <LuRotateCcw className='size-4' aria-hidden />
            <span>{t('layout.reset')}</span>
          </Button>
          <Button
            size='sm'
            onPress={actions.createSession}
          >
            <LuPlus className='size-4' aria-hidden />
            <span>{t('newSession')}</span>
          </Button>
        </div>
      </div>

      <div className='min-h-0 flex-1'>
        <DockviewReact
          className='h-full'
          theme={elysiumDockviewTheme}
          components={panelComponents}
          watermarkComponent={CodingWatermark}
          onReady={onReady}
        />
      </div>
    </div>

    <CreateSessionModal
      key={`create-${formSession}`}
      state={createState}
    />
    <RenameSessionModal
      key={`rename-${formSession}`}
      state={renameState}
      session={selectedSession}
    />
    <DeleteSessionDialog
      state={deleteState}
      session={selectedSession}
    />
  </CodingActionsContext.Provider>
}

// Shown when every panel is closed.
function CodingWatermark() {
  const { t } = useTranslation('coding')

  return <div className='grid h-full place-items-center p-6 text-sm opacity-60'>{
    t('watermark')
  }</div>
}
