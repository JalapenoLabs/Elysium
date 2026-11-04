// Copyright © 2026 Jalapeno Labs

import type { SearchEntry } from '../../search/searchTypes'

// User interface
import { LuInbox, LuList, LuListTodo, LuPlus } from 'react-icons/lu'

// Misc
import { UrlTree } from '../../urls'

// Action items, its inbox and every item, and adding one. See src/search/searchRegistry.ts.
export const searchEntries: SearchEntry[] = [
  {
    id: 'actionItems',
    group: 'pages',
    title: 'actionItems:title',
    tags: 'search:tags.actionItems',
    icon: LuListTodo,
    href: UrlTree.actionItems,
  },
  {
    id: 'inbox',
    group: 'pages',
    title: 'actionItems:nav.inbox',
    tags: 'search:tags.inbox',
    parent: 'actionItems:title',
    icon: LuInbox,
    href: UrlTree.actionItemsInbox,
  },
  {
    id: 'allItems',
    group: 'pages',
    title: 'actionItems:nav.all',
    tags: 'search:tags.allItems',
    parent: 'actionItems:title',
    icon: LuList,
    href: UrlTree.actionItemsAll,
  },
  {
    id: 'newActionItem',
    group: 'actions',
    title: 'actionItems:form.createTitle',
    tags: 'search:tags.newActionItem',
    parent: 'actionItems:title',
    icon: LuPlus,
    href: UrlTree.actionItemsNew,
  },
]
