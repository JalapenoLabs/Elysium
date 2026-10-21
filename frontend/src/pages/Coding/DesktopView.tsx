// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Spinner } from '@heroui/react'

// Utility
import NoVncClient from '@novnc/novnc'

// Misc
import { getDesktopViewUrl } from '../../api/routes/codingSessionRoutes'

type Props = {
  sessionId: number
}

type ViewState = 'connecting' | 'connected' | 'ended'

// A running turn's desktop, watched live. noVNC speaks VNC over the API's WebSocket, which relays
// the turn's view-only VNC server; nothing here can move the agent's mouse or type into its
// screen. The view ends with the turn, and reconnecting picks up the next turn's desktop.
export function DesktopView(props: Props) {
  const { t } = useTranslation('coding')
  const screenRef = useRef<HTMLDivElement>(null)
  const [ viewState, setViewState ] = useState<ViewState>('connecting')
  const [ attempt, setAttempt ] = useState(0)

  useEffect(() => {
    const screen = screenRef.current
    if (!screen) {
      console.debug('DesktopView has no element to draw into')
      return undefined
    }

    setViewState('connecting')
    // No subprotocol: the API's relay carries raw VNC bytes and offers none.
    const client = new NoVncClient(screen, getDesktopViewUrl(props.sessionId), { wsProtocols: []})
    client.viewOnly = true
    client.scaleViewport = true
    client.background = 'transparent'
    client.addEventListener('connect', () => setViewState('connected'))
    client.addEventListener('disconnect', () => setViewState('ended'))

    return () => client.disconnect()
  }, [ props.sessionId, attempt ])

  return <div className='relative aspect-[16/10] w-full overflow-hidden rounded-lg border border-separator bg-surface'>
    <div ref={screenRef} className='h-full w-full' />
    {viewState === 'connecting' && <div className='absolute inset-0 grid place-items-center'>
      <Spinner />
    </div>}
    {viewState === 'ended' && <div className='absolute inset-0 grid place-items-center p-4 text-center'>
      <div>
        <p className='compact text-sm opacity-70'>{t('desktop.ended')}</p>
        <Button size='sm' variant='outline' onPress={() => setAttempt((previous) => previous + 1)}>
          <span>{t('desktop.reconnect')}</span>
        </Button>
      </div>
    </div>}
  </div>
}
