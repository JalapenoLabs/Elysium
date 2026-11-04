// Copyright © 2026 Jalapeno Labs

// Misc
import { UNKNOWN_ROUTE_REDIRECT_TO, UrlTree } from './urls'

// Every page with a fixed address, keyed in lower case because React Router matches paths
// regardless of case. Addresses with a parameter are left out: `/studio/x` may name no item
// at all, so an unknown path is never sent to one.
const pageByLowerCasePath = new Map<string, string>()
for (const path of Object.values(UrlTree)) {
  if (!path.includes(':')) {
    pageByLowerCasePath.set(path.toLowerCase(), path)
  }
}

// Where an unknown path goes: its nearest parent that is a page, so `/settings/llms/x` lands
// on `/settings/llms` and `/studio/x/y` on `/studio` (`~/.claude/docs/ux.md`, Sub-pages).
// A path with no such parent goes to the first page in the sidebar.
export function getUnknownRouteRedirect(pathname: string) {
  const segments = pathname
    .split('/')
    .filter(Boolean)

  // The path itself is unknown, or it would not have reached the catch-all, so the walk
  // starts one level up.
  for (let depth = segments.length - 1; depth > 0; depth--) {
    const parentPath = `/${segments.slice(0, depth).join('/')}`
    const page = pageByLowerCasePath.get(parentPath.toLowerCase())
    if (page) {
      return page
    }
  }

  return UNKNOWN_ROUTE_REDIRECT_TO
}
