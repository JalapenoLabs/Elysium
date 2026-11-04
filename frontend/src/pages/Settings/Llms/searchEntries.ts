// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuBot } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The Claude and Codex credentials agents run on. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'llms',
    group: 'pages',
    title: 'settings:items.llms.title',
    tags: 'search:tags.llms',
    parent: 'settings:title',
    icon: LuBot,
    href: UrlTree.settingsLlms,
  },
]
