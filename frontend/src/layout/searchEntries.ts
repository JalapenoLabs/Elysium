// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../search/searchTypes'

// User interface
import { LuLogOut } from 'react-icons/lu'

// Misc
import { askToSignOut } from '../auth/askToSignOut'

// What the topbar's account menu does, from search too. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'signOut',
    group: 'actions',
    title: 'navigation:topbar.signOut',
    tags: 'search:tags.signOut',
    icon: LuLogOut,
    run: (context) => askToSignOut(context.confirm),
  },
]
