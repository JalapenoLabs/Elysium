// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuBookOpen } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The company doc; the knowledge base is for members. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'companyDoc',
    group: 'pages',
    title: 'settings:items.companyDoc.title',
    tags: 'search:tags.companyDoc',
    parent: 'settings:title',
    icon: LuBookOpen,
    audience: 'members',
    href: UrlTree.settingsCompanyDoc,
  },
]
