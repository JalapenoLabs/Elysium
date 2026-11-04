// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuCodeXml, LuPlus } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// Coding, and starting a session. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'coding',
    group: 'pages',
    title: 'coding:title',
    tags: 'search:tags.coding',
    icon: LuCodeXml,
    href: UrlTree.coding,
  },
  {
    id: 'newCodingSession',
    group: 'actions',
    title: 'coding:create.title',
    tags: 'search:tags.newCodingSession',
    parent: 'coding:title',
    icon: LuPlus,
    href: UrlTree.codingNew,
  },
]
