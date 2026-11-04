// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuListOrdered } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// The worker pool's queue, whose batches quote what was ingested, so members alone; and sizing
// the pool, which admins do. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'workQueue',
    group: 'pages',
    title: 'workQueue:title',
    tags: 'search:tags.workQueue',
    icon: LuListOrdered,
    audience: 'members',
    href: UrlTree.workQueue,
  },
  {
    id: 'workerPool',
    group: 'pages',
    title: 'settings:items.workerPool.title',
    tags: 'search:tags.workerPool',
    parent: 'settings:title',
    icon: LuListOrdered,
    audience: 'admins',
    href: UrlTree.settingsWorkerPool,
  },
]
