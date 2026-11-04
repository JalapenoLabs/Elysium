// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuPlus, LuShapes } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// Studio, and starting an item. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'studio',
    group: 'pages',
    title: 'studio:title',
    tags: 'search:tags.studio',
    icon: LuShapes,
    href: UrlTree.studio,
  },
  {
    id: 'newStudioItem',
    group: 'actions',
    title: 'studio:create.title',
    tags: 'search:tags.newStudioItem',
    parent: 'studio:title',
    icon: LuPlus,
    href: UrlTree.studioNew,
  },
]
