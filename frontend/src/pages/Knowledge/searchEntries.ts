// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuFilePlus, LuLibrary, LuSearch, LuSparkles } from 'react-icons/lu'

// Misc
import { getKnowledgePagesSearchUrl, UrlTree } from '../../urls'

// The knowledge base, its search, and asking it; the knowledge base is for members. Its learnings
// register beside them, and its documents are found by the knowledge search itself
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
    id: 'knowledgeSearch',
    group: 'pages',
    title: 'knowledge:sections.search',
    parent: 'knowledge:title',
    tags: 'search:tags.knowledgeSearch',
    icon: LuSearch,
    audience: 'members',
    href: UrlTree.knowledgeSearch,
  },
  {
    id: 'knowledgeSearchPages',
    group: 'pages',
    title: 'knowledge:search.searchPages',
    parent: 'knowledge:title',
    tags: 'search:tags.knowledgeSearchPages',
    icon: LuSearch,
    audience: 'members',
    href: getKnowledgePagesSearchUrl(),
  },
  {
    id: 'knowledgeAsk',
    group: 'pages',
    title: 'knowledge:sections.ask',
    parent: 'knowledge:title',
    tags: 'search:tags.knowledgeAsk',
    icon: LuSparkles,
    audience: 'members',
    href: UrlTree.knowledgeAsk,
  },
  {
    id: 'newKnowledgePage',
    group: 'actions',
    title: 'knowledge:pages.new',
    parent: 'knowledge:title',
    tags: 'search:tags.newKnowledgePage',
    icon: LuFilePlus,
    audience: 'members',
    href: UrlTree.knowledgePageNew,
  },
]
