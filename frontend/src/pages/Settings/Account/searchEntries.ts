// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// Redux
import { themeChanged } from '../../../store/themeSlice'

// User interface
import { LuMonitor, LuMoon, LuSun, LuUser } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// My account, and the theme it holds, picked from search as it is on the page. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'account',
    group: 'pages',
    title: 'settings:items.account.title',
    tags: 'search:tags.account',
    parent: 'settings:title',
    icon: LuUser,
    href: UrlTree.settingsAccount,
  },
  {
    id: 'themeLight',
    group: 'actions',
    title: 'search:titles.themeLight',
    tags: 'search:tags.theme',
    icon: LuSun,
    run: (context) => context.dispatch(themeChanged('light')),
  },
  {
    id: 'themeDark',
    group: 'actions',
    title: 'search:titles.themeDark',
    tags: 'search:tags.theme',
    icon: LuMoon,
    run: (context) => context.dispatch(themeChanged('dark')),
  },
  {
    id: 'themeSystem',
    group: 'actions',
    title: 'search:titles.themeSystem',
    tags: 'search:tags.theme',
    icon: LuMonitor,
    run: (context) => context.dispatch(themeChanged('system')),
  },
]
