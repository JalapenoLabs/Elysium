// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuWorkflow } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// Workflow runs, which read what was ingested, so members alone. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'runs',
    group: 'pages',
    title: 'runs:title',
    tags: 'search:tags.runs',
    icon: LuWorkflow,
    audience: 'members',
    href: UrlTree.runs,
  },
]
