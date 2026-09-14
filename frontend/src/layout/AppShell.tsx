// Copyright © 2026 Jalapeno Labs

// Core
import { Outlet, useHref, useNavigate } from 'react-router'

// User interface
import { RouterProvider as AriaRouterProvider, Toast } from '@heroui/react'
import { Sidebar } from './Sidebar'
import { Topbar } from './Topbar'

// The persistent frame around every page: navigation on the left, the topbar across
// the content column, and the routed page in the scrollable main area.
export function AppShell() {
  const navigate = useNavigate()

  // HeroUI components are built on React Aria. Handing it the router's navigate
  // makes every `href` on a HeroUI Link or Button a client-side route change
  // instead of a full page load.
  return <AriaRouterProvider navigate={navigate} useHref={useHref}>
    <div className='flex h-dvh overflow-hidden'>
      <Sidebar />
      <div className='flex min-w-0 flex-1 flex-col'>
        <Topbar />
        <main className='flex-1 overflow-y-auto px-10 pt-4 pb-16'>
          <Outlet />
        </main>
      </div>
    </div>
    <Toast.Provider placement='bottom end' />
  </AriaRouterProvider>
}
