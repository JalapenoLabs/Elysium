// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuUsers } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Everyone in the workspace, which admins manage. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'users',
    group: 'pages',
    title: 'settings:items.users.title',
    tags: 'search:tags.users',
    parent: 'settings:title',
    icon: LuUsers,
    audience: 'admins',
    href: UrlTree.settingsUsers,
  },
]
