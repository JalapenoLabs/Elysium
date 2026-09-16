// Copyright © 2026 Jalapeno Labs

import type { IDockviewPanelProps } from 'dockview-react'
import type { ConversationPanelParams } from './codingLayout'

// Core
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectCodingSessionById } from '../../store/codingSessionsSlice'
import { useAppSelector } from '../../store/hooks'
import { selectSatelliteById } from '../../store/satellitesSlice'
import { selectSessionTimeline } from '../../store/sessionEventsSlice'

// User interface
import { Button, Chip, Spinner } from '@heroui/react'
import { ConversationTimeline } from './ConversationTimeline'
import { PromptComposer } from './PromptComposer'

// Misc
import { useCodingSessionsLoader, useSatellitesLoader, useSessionHistoryLoader } from '../../hooks/useServerData'
import { CLOSED_THREAD_STATES, threadStateChipColors, threadStateLabelKeys } from './sessionPresentation'

// One session's conversation: its history from the satellite, live events from the
// event stream, and a composer for the next prompt. Its events stay in Redux only
// while this panel is open.
export function ConversationPanel(props: IDockviewPanelProps<ConversationPanelParams>) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const sessionId = props.params.sessionId

  const session = useAppSelector((state) => selectCodingSessionById(state, sessionId))
  const sessionsStatus = useCodingSessionsLoader()
  useSatellitesLoader()
  const satelliteId = session?.satelliteId ?? ''
  const satellite = useAppSelector((state) => selectSatelliteById(state, satelliteId))
  const timeline = useAppSelector((state) => selectSessionTimeline(state, sessionId))

  const history = useSessionHistoryLoader(sessionId)

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

  const state = session.thread?.state ?? 'unknown'
  const isClosed = CLOSED_THREAD_STATES.includes(state)
  const hasHistory = Boolean(timeline?.isHistoryLoaded)
  const isLoading = !hasHistory && !history.error

  return <div className='flex h-full flex-col'>
    <div className='level shrink-0 border-b border-separator px-4 py-2'>
      <div className='min-w-0'>
        <div className='truncate text-sm font-semibold'>{session.title}</div>
        <div className='truncate text-xs opacity-60'>{
          [ satellite?.name, t('conversation.thread', { number: session.id }) ]
            .filter(Boolean)
            .join(' · ')
        }</div>
      </div>
      <Chip size='sm' variant='soft' color={threadStateChipColors[state]}>{
        t(threadStateLabelKeys[state])
      }</Chip>
    </div>

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
      sessionId={session.id}
      isClosed={isClosed}
    />
  </div>
}
