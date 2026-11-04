// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuKeyRound } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The password vault agents sign in with, which admins manage. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'passwords',
    group: 'pages',
    title: 'settings:items.passwords.title',
    tags: 'search:tags.passwords',
    parent: 'settings:title',
    icon: LuKeyRound,
    audience: 'admins',
    href: UrlTree.settingsPasswords,
  },
]
