// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { Outlet, useHref, useNavigate } from 'react-router'

// UI
import { HeroUIProvider } from '@heroui/react'
import { Splash } from '../pages/Auth/Splash'
import { ConnectionScreen } from '../pages/Connection/ConnectionScreen'

// Redux
import { useAppSelector } from '../store/hooks'

// Misc
import { resolveSession } from '../auth/session'
import { useConnectionGate } from '../connection/useConnectionGate'

export function RootLayout() {
  const navigate = useNavigate()
  const themeMode = useAppSelector((state) => state.theme.mode)

  // In the desktop shell the user must pick which instance to talk to before the
  // app resolves a session; a plain browser is same-origin and skips this.
  const connectionStatus = useConnectionGate()

  // Keep the HeroUI theme class on <html> in sync with Redux state.
  useEffect(() => {
    const root = document.documentElement
    root.classList.remove('light', 'dark')
    root.classList.add(themeMode)
  }, [ themeMode ])

  // Resolve the session once a connection is established (immediately in the
  // browser); the route guards react to the result. Gating this on the
  // connection avoids probing an instance the client hasn't picked yet.
  useEffect(() => {
    if (connectionStatus === 'ready') {
      void resolveSession()
    }
  }, [ connectionStatus ])

  // Providers only: theme, session, and HeroUI routing. The app chrome lives in
  // AppLayout so the full-screen auth screens render without it. Passing
  // navigate/useHref lets HeroUI's Link route client-side via React Router.
  return <HeroUIProvider navigate={navigate} useHref={useHref}>
    <div className='bg-background text-foreground min-h-screen'>
      { connectionStatus === 'checking'
        ? <Splash />
        : connectionStatus === 'needs-connection'
          ? <ConnectionScreen />
          : <Outlet /> }
    </div>
  </HeroUIProvider>
}
