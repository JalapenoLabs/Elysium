// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuPlug, LuPlus } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Connectors, and adding one. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'connectors',
    group: 'pages',
    title: 'settings:items.connectors.title',
    tags: 'search:tags.connectors',
    parent: 'settings:title',
    icon: LuPlug,
    href: UrlTree.settingsConnectors,
  },
  {
    id: 'addConnector',
    group: 'actions',
    title: 'connectors:picker.title',
    tags: 'search:tags.addConnector',
    parent: 'connectors:page.title',
    icon: LuPlus,
    href: UrlTree.settingsConnectorNew,
  },
]
