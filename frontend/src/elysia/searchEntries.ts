// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../search/searchTypes'

// Redux
import { elysiaPanelToggled } from '../store/elysiaSlice'

// User interface
import { LuSparkles } from 'react-icons/lu'

// Opening Elysia's panel, as the topbar's Elysia button does. Asking her about what was typed
// is search's own last row, not an entry. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'openElysia',
    group: 'actions',
    title: 'search:titles.openElysia',
    tags: 'search:tags.openElysia',
    icon: LuSparkles,
    run: (context) => context.dispatch(elysiaPanelToggled(true)),
  },
]
