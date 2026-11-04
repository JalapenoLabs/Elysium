// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuLibrary } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// The Knowledge page; the knowledge base is for members. Its learnings register beside them,
// and its documents are found by the knowledge search itself (`useKnowledgeResults`). See
// src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'knowledge',
    group: 'pages',
    title: 'knowledge:title',
    tags: 'search:tags.knowledge',
    icon: LuLibrary,
    audience: 'members',
    href: UrlTree.knowledge,
  },
]
