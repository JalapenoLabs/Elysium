// Copyright © 2026 Jalapeno Labs

import type { DockviewApi, DockviewReadyEvent } from 'dockview-react'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'
import type { CodingActions } from './codingActionsContext'

// Core
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate, useNavigate, useParams, useSearchParams } from 'react-router'

// Redux
import { codingSessionDeleted, selectCodingSessionById } from '../../store/codingSessionsSlice'
import { useAppDispatch, useAppSelector } from '../../store/hooks'

// User interface
import { buttonVariants, Link, useOverlayState } from '@heroui/react'
import { DockviewReact } from 'dockview-react'
import { LuLayoutPanelLeft, LuPlus, LuRotateCcw } from 'react-icons/lu'
import { PageHeader } from '../../layout/page/PageHeader'
import { ConversationPanel } from './ConversationPanel'
import { RenameSessionModal } from './RenameSessionModal'
import { SessionsPanel } from './SessionsPanel'

// Misc
import { getUpstreamErrorMessage } from '../../api/errors'
import { deleteCodingSession } from '../../api/routes/codingSessionRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { useCodingSessionsLoader } from '../../hooks/useServerData'
import { getNewCodingSessionUrl, NEW_SESSION_ITEM_PARAM, UrlTree } from '../../urls'
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
import { toast } from '../../toast/toast'

const panelComponents = {
  [CODING_PANEL_COMPONENTS.sessions]: SessionsPanel,
  [CODING_PANEL_COMPONENTS.conversation]: ConversationPanel,
}

// The Coding area: a Dockview workspace holding the sessions overview and one
// conversation panel per open session, arranged however the user drags them. The
// layout persists per browser. This page owns the dialogs; panels open them through
// CodingActionsContext. `/coding/<number>` opens that session's conversation once the
// workspace and the session are both loaded, then returns to `/coding`. New session is its
// own page, `/coding/new`, where a link to `/coding?item=<id>` lands too.
export function CodingPage() {
  const { t } = useTranslation([ 'coding', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const dockviewApiRef = useRef<DockviewApi | null>(null)
  const [ isDockviewReady, setIsDockviewReady ] = useState(false)
  const navigate = useNavigate()
  const params = useParams()
  const [ searchParams ] = useSearchParams()
  const requestedItemId = searchParams.get(NEW_SESSION_ITEM_PARAM)
  const requestedSessionId = Number(params.sessionId)
  const sessionsStatus = useCodingSessionsLoader()
  const requestedSession = useAppSelector((state) => selectCodingSessionById(state, requestedSessionId))

  const renameState = useOverlayState()
  // useOverlayState answers a new object every render; only its functions are stable. The
  // actions depend on those alone, so effects that call an action run once, not every render.
  const openRename = renameState.open
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
    renameSession: (session) => {
      setSelectedSession(session)
      setFormSession((formKey) => formKey + 1)
      openRename()
    },
    deleteSession: (session) => confirm({
      title: t('delete.title', { title: session.title }),
      message: t('delete.body', { title: session.title }),
      isIrreversible: true,
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteCodingSession(session.id)
        }
        catch (error) {
          const message = getUpstreamErrorMessage(error)
          if (!message) {
            console.debug('CodingPage failed to delete the session', { error, sessionId: session.id })
          }
          toast.danger(message ?? t('common:errors.unexpected'))
          throw error
        }
        dispatch(codingSessionDeleted(session.id))
        toast.success(t('toasts.deleted', { title: session.title }))
      },
    }),
  }), [ openRename, confirm, dispatch, t ])

  useEffect(() => {
    if (params.sessionId === undefined || !isDockviewReady || !dockviewApiRef.current) {
      return
    }
    if (requestedSession) {
      openConversation(dockviewApiRef.current, requestedSession)
    }
    else if (sessionsStatus === 'loading') {
      return
    }
    else {
      console.debug('CodingPage was linked to a session it cannot find', { sessionId: params.sessionId })
    }
    navigate(UrlTree.coding, { replace: true })
  }, [ params.sessionId, isDockviewReady, requestedSession, sessionsStatus, navigate ])


  function onReady(event: DockviewReadyEvent) {
    dockviewApiRef.current = event.api
    setIsDockviewReady(true)

    if (!restoreLayout(event.api)) {
      showSessionsPanel(event.api, t('panels.sessions'))
    }
    event.api.onDidLayoutChange(() => saveLayout(event.api))
  }

  function showSessions() {
    if (!dockviewApiRef.current) {
      console.debug('CodingPage cannot show the sessions panel before Dockview is ready')
      return
    }
    showSessionsPanel(dockviewApiRef.current, t('panels.sessions'))
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

  // A link to `/coding?item=` still lands on New session, which is a page of its own.
  if (requestedItemId) {
    return <Navigate to={getNewCodingSessionUrl(requestedItemId)} replace />
  }

  return <CodingActionsContext.Provider value={actions}>
    <div className='flex h-full flex-col'>
      <PageHeader
        className='shrink-0 border-b border-separator px-4 py-3 md:px-6'
        title={t('title')}
        primaryAction={<Link href={UrlTree.codingNew} className={buttonVariants()}>
          <LuPlus className='size-4' aria-hidden />
          <span>{t('newSession')}</span>
        </Link>}
        actions={[
          {
            id: 'showSessions',
            label: t('panels.sessions'),
            icon: LuLayoutPanelLeft,
            onAction: showSessions,
          },
          {
            id: 'resetLayout',
            label: t('layout.reset'),
            icon: LuRotateCcw,
            onAction: resetLayout,
          },
        ]}
      />

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

    <RenameSessionModal
      key={`rename-${formSession}`}
      state={renameState}
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
