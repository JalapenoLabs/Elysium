// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from './searchTypes'

// Every page and action search can reach. Each part of the app registers its own in a
// `searchEntries.ts` beside it, exporting `searchEntries`, and Vite gathers every such file
// at build time, so a new page shows up in search by adding that file, with no central list
// to edit. The files hold only links, icons, and translation keys, never a page component,
// since they load with the app.
const registryFiles = import.meta.glob<SearchEntry[]>(
  '../**/searchEntries.ts',
  {
    eager: true,
    import: 'searchEntries',
  },
)

export const registeredSearchEntries: SearchEntry[] = Object.values(registryFiles).flat()
