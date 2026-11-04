// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuBrainCircuit } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The model and effort each step and agent runs on, which admins choose. See
// src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'models',
    group: 'pages',
    title: 'settings:items.models.title',
    tags: 'search:tags.models',
    parent: 'settings:title',
    icon: LuBrainCircuit,
    audience: 'admins',
    href: UrlTree.settingsModels,
  },
]
