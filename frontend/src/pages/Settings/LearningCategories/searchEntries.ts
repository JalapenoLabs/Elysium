// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuPlus, LuTags } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The workspace's own learning categories, which admins manage. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'learningCategories',
    group: 'pages',
    title: 'settings:items.learningCategories.title',
    tags: 'search:tags.learningCategories',
    parent: 'settings:title',
    icon: LuTags,
    audience: 'admins',
    href: UrlTree.settingsLearningCategories,
  },
  {
    id: 'newLearningCategory',
    group: 'actions',
    title: 'learningCategories:newTitle',
    tags: 'search:tags.newLearningCategory',
    parent: 'learningCategories:title',
    icon: LuPlus,
    audience: 'admins',
    href: UrlTree.settingsLearningCategoryNew,
  },
]
