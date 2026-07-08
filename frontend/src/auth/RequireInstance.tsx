// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { Navigate } from 'react-router'

// Redux
import { useAppSelector } from '../store/hooks'

// Misc
import { UrlTree } from '../urls'

type Props = {
  children: ReactNode
}

// Forces first-run setup when the instance has not been claimed yet. Stays a
// pass-through until the first-run check resolves `instanceClaimed` to `false`
// (the backend context probe lands with the API work).
export function RequireInstance(props: Props) {
  const instanceClaimed = useAppSelector((state) => state.session.instanceClaimed)

  if (instanceClaimed === false) {
    console.debug('RequireInstance: instance not yet claimed, redirecting to first-run setup')
    return <Navigate to={UrlTree.setup} replace />
  }

  return props.children
}
