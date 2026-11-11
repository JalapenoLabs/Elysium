// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuVideo } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Where meetings come from. The meetings themselves register with the knowledge base. See
// src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'meetingSources',
    group: 'pages',
    title: 'settings:items.meetings.title',
    tags: 'search:tags.meetingSources',
    parent: 'settings:title',
    icon: LuVideo,
    href: UrlTree.settingsMeetings,
  },
]
