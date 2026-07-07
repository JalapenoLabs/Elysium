// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect } from 'react'
import { Outlet, useHref, useNavigate } from 'react-router'

// UI
import { HeroUIProvider } from '@heroui/react'
import { NavBar } from './NavBar'

// Redux
import { useAppSelector } from '../store/hooks'

export function RootLayout() {
  const navigate = useNavigate()
  const themeMode = useAppSelector((state) => state.theme.mode)

  // Keep the HeroUI theme class on <html> in sync with Redux state.
  useEffect(() => {
    const root = document.documentElement
    root.classList.remove('light', 'dark')
    root.classList.add(themeMode)
  }, [ themeMode ])

  // Passing navigate/useHref lets HeroUI's Link route client-side via React Router.
  return <HeroUIProvider navigate={navigate} useHref={useHref}>
    <div className='bg-background text-foreground min-h-screen'>
      <NavBar />
      <Outlet />
    </div>
  </HeroUIProvider>
}
