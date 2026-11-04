// Copyright © 2026 Jalapeno Labs

// Core
import { Navigate, useLocation } from 'react-router'

// Misc
import { getUnknownRouteRedirect } from '../unknownRoutes'

// The catch-all route's element: sends an address no route matches one level up, to its
// nearest parent page. Replacing the entry keeps the unknown address out of the history, so
// Back does not return to it.
export function UnknownRouteRedirect() {
  const location = useLocation()

  console.debug('No route matches this address, redirecting to its nearest parent page', {
    pathname: location.pathname,
  })

  return <Navigate to={getUnknownRouteRedirect(location.pathname)} replace />
}
