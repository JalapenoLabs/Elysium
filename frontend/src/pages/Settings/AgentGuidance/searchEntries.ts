// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuScrollText } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The agent guidance; the knowledge base is for members. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'agentGuidance',
    group: 'pages',
    title: 'settings:items.agentGuidance.title',
    tags: 'search:tags.agentGuidance',
    parent: 'settings:title',
    icon: LuScrollText,
    audience: 'members',
    href: UrlTree.settingsAgentGuidance,
  },
]
