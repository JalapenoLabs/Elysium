// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'

// User interface
import {
  LuCodeXml,
  LuFolderKanban,
  LuLibrary,
  LuListOrdered,
  LuListTodo,
  LuShapes,
  LuWorkflow,
} from 'react-icons/lu'

// Misc
import { UrlTree } from '../urls'

export type NavigationItem = {
  labelKey: ParseKeys<'navigation'>
  href: string
  icon: IconType
  // True matches the path exactly, so an entry does not highlight on its nested routes.
  end: boolean
  // True for areas a guest never reaches, which read what was ingested, as the knowledge base
  // does; the link is left out for them.
  membersOnly: boolean
}

// Sidebar entries, in display order. New top-level areas are added here.
export const primaryNavigation: NavigationItem[] = [
  {
    labelKey: 'primary.actionItems',
    href: UrlTree.actionItems,
    icon: LuListTodo,
    end: false,
    membersOnly: false,
  },
  {
    labelKey: 'primary.projects',
    href: UrlTree.projects,
    icon: LuFolderKanban,
    end: false,
    membersOnly: false,
  },
  {
    labelKey: 'primary.coding',
    href: UrlTree.coding,
    icon: LuCodeXml,
    end: false,
    membersOnly: false,
  },
  {
    labelKey: 'primary.studio',
    href: UrlTree.studio,
    icon: LuShapes,
    end: false,
    membersOnly: false,
  },
  {
    labelKey: 'primary.knowledge',
    href: UrlTree.knowledge,
    icon: LuLibrary,
    end: false,
    membersOnly: true,
  },
  {
    labelKey: 'primary.runs',
    href: UrlTree.runs,
    icon: LuWorkflow,
    end: false,
    membersOnly: true,
  },
  {
    labelKey: 'primary.workQueue',
    href: UrlTree.workQueue,
    icon: LuListOrdered,
    end: false,
    membersOnly: true,
  },
]
