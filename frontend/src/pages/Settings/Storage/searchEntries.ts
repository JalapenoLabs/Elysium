// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../../search/searchTypes'

// User interface
import { LuHardDrive, LuPaperclip, LuPlus } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../../urls'

// Storage locations, and adding one. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'storage',
    group: 'pages',
    title: 'settings:items.storage.title',
    tags: 'search:tags.storage',
    parent: 'settings:title',
    icon: LuHardDrive,
    href: UrlTree.settingsStorage,
  },
  {
    id: 'addStorageLocation',
    group: 'actions',
    title: 'storage:form.createTitle',
    tags: 'search:tags.addStorageLocation',
    parent: 'storage:title',
    icon: LuPlus,
    href: UrlTree.settingsStorageNew,
  },
  {
    id: 'attachmentDestination',
    group: 'pages',
    title: 'storage:attachments.title',
    tags: 'search:tags.attachmentDestination',
    parent: 'storage:title',
    icon: LuPaperclip,
    href: UrlTree.settingsStorageAttachments,
  },
]
