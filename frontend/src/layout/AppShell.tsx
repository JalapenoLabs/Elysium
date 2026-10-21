// Copyright © 2026 Jalapeno Labs

// Core
import { Outlet, useHref, useMatches, useNavigate } from 'react-router'

// Redux
import { useAppSelector } from '../store/hooks'
import { selectIsElysiaPanelOpen } from '../store/elysiaSlice'

// User interface
import { RouterProvider as AriaRouterProvider, Toast } from '@heroui/react'
import { ElysiaPanel } from '../elysia/ElysiaPanel'
import { Sidebar } from './Sidebar'
import { Topbar } from './Topbar'

// Misc
import { useElysiaSounds } from '../elysia/useElysiaSounds'

// Pages lay out by viewport breakpoints, so a 26rem panel beside them would leave a
// 1280px screen with a 640px page laid out for 1280px. Only from 2xl does it sit beside
// the page; below that it floats over the page's right edge, and below sm it takes the
// whole width.
const ELYSIA_PANEL_PLACEMENT = [
  'fixed inset-y-0 right-0 z-40 w-full shadow-xl sm:w-[26rem]',
  '2xl:static 2xl:z-auto 2xl:shrink-0 2xl:shadow-none',
].join(' ')

// The persistent frame around every page: navigation on the left, the topbar across
// the content column, the routed page in the scrollable main area, and Elysia's panel on
// the right while it is open.
export function AppShell() {
  const navigate = useNavigate()
  const matches = useMatches()
  const isWorkspace = matches.some((match) => isWorkspaceHandle(match.handle))
  const isElysiaOpen = useAppSelector(selectIsElysiaPanelOpen)
  useElysiaSounds()

  // HeroUI components are built on React Aria. Handing it the router's navigate
  // makes every `href` on a HeroUI Link or Button a client-side route change
  // instead of a full page load.
  return <AriaRouterProvider navigate={navigate} useHref={useHref}>
    <div className='flex h-dvh overflow-hidden'>
      <Sidebar />
      <div className='flex min-w-0 flex-1 flex-col'>
        <Topbar />
        <main className={
          isWorkspace
            ? 'min-h-0 flex-1 overflow-hidden border-t border-separator'
            : 'flex-1 overflow-y-auto px-4 pt-4 pb-16 md:px-10'
        }>
          <Outlet />
        </main>
      </div>
      {isElysiaOpen && <div className={ELYSIA_PANEL_PLACEMENT}>
        <ElysiaPanel />
      </div>}
    </div>
    <Toast.Provider placement='bottom end' />
  </AriaRouterProvider>
}

// Route handles are untyped in React Router; this narrows the one AppShell reads.
function isWorkspaceHandle(handle: unknown) {
  return typeof handle === 'object'
    && handle !== null
    && 'layout' in handle
    && handle.layout === 'workspace'
}
