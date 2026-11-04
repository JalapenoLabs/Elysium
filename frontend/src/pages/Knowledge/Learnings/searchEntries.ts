// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuLightbulb, LuPlus } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Learnings, and saving one; the knowledge base is for members. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'learnings',
    group: 'pages',
    title: 'learnings:title',
    tags: 'search:tags.learnings',
    parent: 'knowledge:title',
    icon: LuLightbulb,
    audience: 'members',
    href: UrlTree.knowledgeLearnings,
  },
  {
    id: 'newLearning',
    group: 'actions',
    title: 'learnings:new',
    tags: 'search:tags.newLearning',
    parent: 'learnings:title',
    icon: LuPlus,
    audience: 'members',
    href: UrlTree.knowledgeLearningNew,
  },
]
