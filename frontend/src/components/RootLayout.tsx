// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { Outlet, useHref, useNavigate } from 'react-router'

// UI
import { HeroUIProvider } from '@heroui/react'

// Redux
import { useAppSelector } from '../store/hooks'

// Misc
import { resolveSession } from '../auth/session'

export function RootLayout() {
  const navigate = useNavigate()
  const themeMode = useAppSelector((state) => state.theme.mode)

  // Keep the HeroUI theme class on <html> in sync with Redux state.
  useEffect(() => {
    const root = document.documentElement
    root.classList.remove('light', 'dark')
    root.classList.add(themeMode)
  }, [ themeMode ])

  // Resolve the session once on load; the route guards react to the result.
  useEffect(() => {
    void resolveSession()
  }, [])

  // Providers only: theme, session, and HeroUI routing. The app chrome (NavBar)
  // lives in AppLayout so the full-screen auth screens can render without it.
  // Passing navigate/useHref lets HeroUI's Link route client-side via React Router.
  return <HeroUIProvider navigate={navigate} useHref={useHref}>
    <div className='bg-background text-foreground min-h-screen'>
      <Outlet />
    </div>
  </HeroUIProvider>
}
