// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuFolderKanban, LuPlus } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// Projects, and creating one. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'projects',
    group: 'pages',
    title: 'projects:title',
    tags: 'search:tags.projects',
    icon: LuFolderKanban,
    href: UrlTree.projects,
  },
  {
    id: 'newProject',
    group: 'actions',
    title: 'projects:form.createTitle',
    tags: 'search:tags.newProject',
    parent: 'projects:title',
    icon: LuPlus,
    href: UrlTree.projectsNew,
  },
]
