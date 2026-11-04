// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuSettings } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// The settings directory itself; each settings page registers its own. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'settings',
    group: 'pages',
    title: 'settings:title',
    tags: 'search:tags.settings',
    icon: LuSettings,
    href: UrlTree.settings,
  },
]
