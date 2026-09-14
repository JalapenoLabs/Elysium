// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'

// User interface
import { LuHouse } from 'react-icons/lu'

// Misc
import { UrlTree } from '../urls'

export type NavigationItem = {
  labelKey: ParseKeys<'navigation'>
  href: string
  icon: IconType
  // Exact matching keeps Home from highlighting on every nested route.
  end: boolean
}

// Sidebar entries, in display order. New top-level areas are added here.
export const primaryNavigation: NavigationItem[] = [
  {
    labelKey: 'primary.home',
    href: UrlTree.root,
    icon: LuHouse,
    end: true,
  },
]
