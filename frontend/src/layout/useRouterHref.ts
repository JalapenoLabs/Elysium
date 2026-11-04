// Copyright © 2026 Jalapeno Labs

// Core
import { useHref } from 'react-router'

// What every HeroUI `Link` and link `Button` writes as its `href`, handed to React Aria's
// `RouterProvider`. React Router resolves any href as a path in the app, which turns a link
// to another site into `/https:/...`; a full URL keeps its own address instead.
export function useRouterHref(href: string) {
  // Called on every render, as a hook must be, whichever address is used.
  const routedHref = useHref(href)
  if (URL.canParse(href)) {
    return href
  }
  return routedHref
}
