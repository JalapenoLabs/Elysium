// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuFileDiff } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// The changesets waiting for review. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'changesets',
    group: 'pages',
    title: 'changesets:title',
    tags: 'search:tags.changesets',
    parent: 'actionItems:title',
    icon: LuFileDiff,
    href: UrlTree.changesets,
  },
]
