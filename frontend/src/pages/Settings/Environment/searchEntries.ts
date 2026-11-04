// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuPlus, LuVariable } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Environment variables, and adding one. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'environment',
    group: 'pages',
    title: 'settings:items.environment.title',
    tags: 'search:tags.environment',
    parent: 'settings:title',
    icon: LuVariable,
    href: UrlTree.settingsEnvironment,
  },
  {
    id: 'addEnvironmentVariable',
    group: 'actions',
    title: 'environment:form.createTitle',
    tags: 'search:tags.addEnvironmentVariable',
    parent: 'environment:title',
    icon: LuPlus,
    href: UrlTree.settingsEnvironmentNew,
  },
]
