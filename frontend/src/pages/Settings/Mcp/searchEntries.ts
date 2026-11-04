// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuCable } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// The MCP server and its connected clients. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'mcp',
    group: 'pages',
    title: 'settings:items.mcp.title',
    tags: 'search:tags.mcp',
    parent: 'settings:title',
    icon: LuCable,
    href: UrlTree.settingsMcp,
  },
]
