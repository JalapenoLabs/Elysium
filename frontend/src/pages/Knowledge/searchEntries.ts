// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuLibrary, LuSparkles } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// The Knowledge page and its Ask tab; the knowledge base is for members. Its learnings register
// beside them, and its documents are found by the knowledge search itself
// (`useKnowledgeResults`). See src/search/searchRegistry.ts.
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
  {
    id: 'knowledgeAsk',
    group: 'pages',
    title: 'knowledge:tabs.ask',
    parent: 'knowledge:title',
    tags: 'search:tags.knowledgeAsk',
    icon: LuSparkles,
    audience: 'members',
    href: UrlTree.knowledgeAsk,
  },
]
