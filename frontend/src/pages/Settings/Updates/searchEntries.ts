// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuRefreshCw } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Auto-update, which admins manage. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'updates',
    group: 'pages',
    title: 'settings:items.updates.title',
    tags: 'search:tags.updates',
    parent: 'settings:title',
    icon: LuRefreshCw,
    audience: 'admins',
    href: UrlTree.settingsUpdates,
  },
]
