// Copyright © 2026 Jalapeno Labs

import type { IDockviewPanelProps } from 'dockview-react'
import type { ConversationPanelParams } from './codingLayout'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectCodingSessionById } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectSessionTimeline } from '../../store/sessionEventsSlice'

// User interface
import { Button, Chip, Spinner, toast } from '@heroui/react'
import { ConversationTimeline } from './ConversationTimeline'
import { DesktopView } from './DesktopView'
import { PromptComposer } from './PromptComposer'
import { SessionActionItemLink } from './SessionActionItemLink'

// Misc
import { getUpstreamErrorMessage } from '../../api/errors'
import { startTurn } from '../../api/routes/codingSessionRoutes'
import { useCodingSessionsLoader, useSessionHistoryLoader } from '../../hooks/useServerData'
import { activityStateChipColors, activityStateLabelKeys } from './sessionPresentation'

// One session's conversation: its kept history, live events from the event stream, and a
// composer for the next prompt. Its events stay in Redux only
// while this panel is open.
export function ConversationPanel(props: IDockviewPanelProps<ConversationPanelParams>) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const sessionId = props.params.sessionId

  const session = useAppSelector((state) => selectCodingSessionById(state, sessionId))
  const sessionsStatus = useCodingSessionsLoader()
  const timeline = useAppSelector((state) => selectSessionTimeline(state, sessionId))

  const history = useSessionHistoryLoader(sessionId)
  const [ isWatchingDesktop, setIsWatchingDesktop ] = useState(false)

  const title = session?.title
  useEffect(() => {
    if (title) {
      props.api.setTitle(title)
    }
  }, [ props.api, title ])

  if (!session) {
    if (sessionsStatus !== 'loaded') {
      return <div className='grid h-full place-items-center'>
        <Spinner />
      </div>
    }
    return <div className='grid h-full place-items-center p-6 text-sm opacity-70'>{
      t('conversation.missing')
    }</div>
  }

  const state = session.activity.state
  const hasHistory = Boolean(timeline?.isHistoryLoaded)
  const isLoading = !hasHistory && !history.error

  async function sendPrompt(prompt: string) {
    try {
      await startTurn(sessionId, prompt)
      return true
    }
    catch (error) {
      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('ConversationPanel failed to start a turn', { error, sessionId })
      }
      toast.danger(t('toasts.promptFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
      return false
    }
  }

  return <div className='flex h-full flex-col'>
    <div className='level shrink-0 border-b border-separator px-4 py-2'>
      <div className='min-w-0'>
        <div className='truncate text-sm font-semibold'>{session.title}</div>
        <div className='truncate text-xs opacity-60'>{
          t('conversation.number', { number: session.id })
        }</div>
        {session.actionItemId && <div className='truncate text-xs'>
          <SessionActionItemLink actionItemId={session.actionItemId} />
        </div>}
      </div>
      <div className='flex shrink-0 items-center gap-2'>
        <Button
          size='sm'
          variant='outline'
          isDisabled={state !== 'running' && !isWatchingDesktop}
          onPress={() => setIsWatchingDesktop((previous) => !previous)}
        >
          <span>{
            isWatchingDesktop
              ? t('desktop.hide')
              : t('desktop.watch')
          }</span>
        </Button>
        <Chip size='sm' variant='soft' color={activityStateChipColors[state]}>{
          t(activityStateLabelKeys[state])
        }</Chip>
      </div>
    </div>

    {isWatchingDesktop && <div className='shrink-0 border-b border-separator p-3'>
      <DesktopView sessionId={sessionId} />
    </div>}

    <div className='min-h-0 flex-1'>
      {isLoading && <div className='grid h-full place-items-center'>
        <div className='text-center text-sm opacity-70'>
          <Spinner className='mx-auto compact' />
          <span>{t('conversation.loading')}</span>
        </div>
      </div>}

      {!hasHistory && Boolean(history.error) && <div className='grid h-full place-items-center p-6'>
        <div className='text-center'>
          <p className='compact text-sm text-danger'>{
            t('conversation.loadError')
          }</p>
          <Button size='sm' variant='outline' onPress={history.retry}>
            <span>{t('common:actions.retry')}</span>
          </Button>
        </div>
      </div>}

      {hasHistory && timeline && <ConversationTimeline
        timeline={timeline}
      />}
    </div>

    <PromptComposer
      onSend={sendPrompt}
    />
  </div>
}
