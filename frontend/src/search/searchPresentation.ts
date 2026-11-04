// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { SearchGroup } from './searchTypes'

// Each kind of result's section heading.
export const searchGroupLabelKeys = {
  pages: 'groups.pages',
  actions: 'groups.actions',
} as const satisfies Record<SearchGroup, ParseKeys<'search'>>
