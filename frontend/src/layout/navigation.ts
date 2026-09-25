// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'

// User interface
import { LuCodeXml, LuFolderKanban, LuListTodo } from 'react-icons/lu'

// Misc
import { UrlTree } from '../urls'

export type NavigationItem = {
  labelKey: ParseKeys<'navigation'>
  href: string
  icon: IconType
  // True matches the path exactly, so an entry does not highlight on its nested routes.
  end: boolean
}

// Sidebar entries, in display order. New top-level areas are added here.
export const primaryNavigation: NavigationItem[] = [
  {
    labelKey: 'primary.actionItems',
    href: UrlTree.actionItems,
    icon: LuListTodo,
    end: false,
  },
  {
    labelKey: 'primary.projects',
    href: UrlTree.projects,
    icon: LuFolderKanban,
    end: false,
  },
  {
    labelKey: 'primary.coding',
    href: UrlTree.coding,
    icon: LuCodeXml,
    end: false,
  },
]
