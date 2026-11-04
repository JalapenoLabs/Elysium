// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuMail } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The mail server and every mailbox. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'email',
    group: 'pages',
    title: 'settings:items.email.title',
    tags: 'search:tags.email',
    parent: 'settings:title',
    icon: LuMail,
    href: UrlTree.settingsEmail,
  },
]
