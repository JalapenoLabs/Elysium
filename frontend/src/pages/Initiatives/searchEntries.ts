// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuPlus, LuTarget } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// Initiatives, and starting one. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'initiatives',
    group: 'pages',
    title: 'initiatives:title',
    tags: 'search:tags.initiatives',
    parent: 'actionItems:title',
    icon: LuTarget,
    href: UrlTree.initiatives,
  },
  {
    id: 'newInitiative',
    group: 'actions',
    title: 'initiatives:form.createTitle',
    tags: 'search:tags.newInitiative',
    parent: 'initiatives:title',
    icon: LuPlus,
    href: UrlTree.initiativesNew,
  },
]
