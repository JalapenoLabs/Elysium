// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useNavigate } from 'react-router'

// User interface
import { RouterProvider } from '@heroui/react'

// Misc
import { useRouterHref } from './layout/useRouterHref'

type Props = {
  children: ReactNode
}

// For tests rendering inside a React Router: hands React Aria the router, as `AppShell` does,
// so a HeroUI or React Aria link routes in the app rather than asking jsdom to load a page.
export function TestAriaRouter(props: Props) {
  const navigate = useNavigate()

  return <RouterProvider navigate={navigate} useHref={useRouterHref}>{
    props.children
  }</RouterProvider>
}
