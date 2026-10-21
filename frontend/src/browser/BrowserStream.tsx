// Copyright © 2026 Jalapeno Labs

import type { ConnectionState, RemoteCursor, ScreenSize } from './nekoConnection'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Spinner } from '@heroui/react'

// Misc
import { getBrowserStreamUrl } from '../api/routes/browserSessionRoutes'
import { toKeysym } from './keysyms'
import { NekoConnection } from './nekoConnection'
import { pointerButtonByMouseButton } from './nekoInput'
import { toDesktopPoint, toScrollSteps } from './pointer'

type Props = {
  browserSessionId: string
}

// Neko's default desktop, until the browser says otherwise.
const DEFAULT_SCREEN: ScreenSize = { width: 1920, height: 1080 }

// A browser the host runs, live: the picture, and everything typed, clicked, scrolled, and
// pasted on it sent back. Focus it to type into it.
export function BrowserStream(props: Props) {
  const { t } = useTranslation('browsers')
  const videoRef = useRef<HTMLVideoElement>(null)
  const connectionRef = useRef<NekoConnection | null>(null)
  const [ state, setState ] = useState<ConnectionState>('connecting')
  const [ screen, setScreen ] = useState<ScreenSize>(DEFAULT_SCREEN)
  const [ cursor, setCursor ] = useState<RemoteCursor | null>(null)
  // Bumping it opens a new connection, which is how "Reconnect" works.
  const [ attempt, setAttempt ] = useState(0)
  // Wheel travel short of a whole scroll step, per axis, kept for the next wheel event.
  const scrollCarryRef = useRef({ x: 0, y: 0 })

  useEffect(() => {
    const connection = new NekoConnection(getBrowserStreamUrl(props.browserSessionId), {
      onState: setState,
      onScreenSize: setScreen,
      onCursor: setCursor,
      onStream: (stream) => {
        if (videoRef.current) {
          videoRef.current.srcObject = stream
        }
      },
    })
    connectionRef.current = connection
    connection.open()
    return () => {
      connection.close()
      connectionRef.current = null
    }
  }, [ props.browserSessionId, attempt ])

  function desktopPoint(event: React.MouseEvent<HTMLVideoElement>) {
    const bounds = event.currentTarget.getBoundingClientRect()
    return toDesktopPoint({
      x: event.clientX - bounds.left,
      y: event.clientY - bounds.top,
      width: bounds.width,
      height: bounds.height,
    }, screen)
  }

  function sendKey(event: React.KeyboardEvent, isDown: boolean) {
    const keysym = toKeysym(event)
    if (keysym === null) {
      return
    }
    // Every key belongs to the remote page while the stream has focus, Tab included.
    event.preventDefault()
    connectionRef.current?.key(keysym, isDown)
  }

  const cursorStyle = cursor
    ? `url(${cursor.url}) ${cursor.hotX} ${cursor.hotY}, default`
    : 'default'

  return <div className='relative w-full overflow-hidden rounded-lg bg-black'>
    <video
      ref={videoRef}
      autoPlay
      muted
      playsInline
      tabIndex={0}
      aria-label={t('stream.label')}
      className='block aspect-video w-full object-contain outline-none focus-visible:ring-2 focus-visible:ring-accent'
      style={{ cursor: cursorStyle }}
      onMouseMove={(event) => {
        const point = desktopPoint(event)
        connectionRef.current?.move(point.x, point.y)
      }}
      // Capturing the pointer while a button is down keeps its release, and the moves of a
      // drag, coming to the stream when the pointer leaves it; the release would otherwise
      // never reach the remote desktop.
      onPointerDown={(event) => event.currentTarget.setPointerCapture(event.pointerId)}
      onLostPointerCapture={() => connectionRef.current?.releaseButtons()}
      // Focus moving away takes the releases of whatever is held with it: Alt+Tab would
      // otherwise leave Alt down on the remote page.
      onBlur={() => connectionRef.current?.releaseAll()}
      onMouseDown={(event) => {
        event.currentTarget.focus()
        const button = pointerButtonByMouseButton[event.button]
        if (button !== undefined) {
          connectionRef.current?.button(button, true)
        }
      }}
      onMouseUp={(event) => {
        const button = pointerButtonByMouseButton[event.button]
        if (button !== undefined) {
          connectionRef.current?.button(button, false)
        }
      }}
      onContextMenu={(event) => event.preventDefault()}
      onWheel={(event) => {
        const carry = scrollCarryRef.current
        const horizontal = toScrollSteps(event.deltaX, event.deltaMode, carry.x)
        const vertical = toScrollSteps(event.deltaY, event.deltaMode, carry.y)
        scrollCarryRef.current = { x: horizontal.carry, y: vertical.carry }
        if (horizontal.steps || vertical.steps) {
          connectionRef.current?.scroll(horizontal.steps, vertical.steps, event.ctrlKey)
        }
      }}
      onKeyDown={(event) => {
        // The viewer's own paste shortcut arrives as a paste event, with the text.
        const isPasteShortcut = (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v'
        if (isPasteShortcut) {
          return
        }
        sendKey(event, true)
      }}
      onKeyUp={(event) => sendKey(event, false)}
      onPaste={(event) => {
        event.preventDefault()
        const text = event.clipboardData.getData('text/plain')
        if (text) {
          void connectionRef.current?.paste(text)
        }
      }}
    />

    {state === 'connecting' && <div className='absolute inset-0 grid place-items-center'>
      <Spinner aria-label={t('stream.connecting')} />
    </div>}

    {state === 'disconnected' && <div className='absolute inset-0 grid place-items-center bg-black/70'>
      <div className='text-center'>
        <p className='compact text-sm text-white opacity-80'>{
          t('stream.disconnected')
        }</p>
        <Button size='sm' onPress={() => setAttempt((previous) => previous + 1)}>
          <span>{t('stream.reconnect')}</span>
        </Button>
      </div>
    </div>}
  </div>
}
