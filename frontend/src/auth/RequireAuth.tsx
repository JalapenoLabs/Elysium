// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { Navigate, useLocation } from 'react-router'

// Redux
import { useAppSelector } from '../store/hooks'

// UI
import { Splash } from '../pages/Auth/Splash'

// Misc
import { UrlTree } from '../urls'

type Props = {
  children: ReactNode
}

// Gates protected routes on the resolved session. While the session is still
// resolving it shows the splash; an unreachable identity service routes to a
// degraded state rather than silently showing an anonymous view.
export function RequireAuth(props: Props) {
  const status = useAppSelector((state) => state.session.status)
  const location = useLocation()

  if (status === 'authenticated') {
    return props.children
  }
  if (status === 'error') {
    console.debug('RequireAuth: identity service unreachable, redirecting to identity-unavailable')
    return <Navigate to={UrlTree.identityUnavailable} replace />
  }
  if (status === 'anonymous') {
    console.debug('RequireAuth: no session, redirecting to sign-in', { from: location.pathname })
    return <Navigate to={UrlTree.signIn} replace state={{ from: location.pathname }} />
  }

  // unknown | loading
  return <Splash />
}
