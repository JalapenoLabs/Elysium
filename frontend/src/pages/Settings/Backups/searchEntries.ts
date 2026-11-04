// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuDatabaseBackup } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Database backups, which admins manage. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'backups',
    group: 'pages',
    title: 'settings:items.backups.title',
    tags: 'search:tags.backups',
    parent: 'settings:title',
    icon: LuDatabaseBackup,
    audience: 'admins',
    href: UrlTree.settingsBackups,
  },
]
