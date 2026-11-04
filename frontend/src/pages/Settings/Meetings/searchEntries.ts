// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuVideo } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Meeting sources and recent meetings. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'meetings',
    group: 'pages',
    title: 'settings:items.meetings.title',
    tags: 'search:tags.meetings',
    parent: 'settings:title',
    icon: LuVideo,
    href: UrlTree.settingsMeetings,
  },
]
