// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { SearchGroup } from './searchTypes'

// Each kind of result's section heading, and its filter on the search page.
export const searchGroupLabelKeys = {
  pages: 'groups.pages',
  actions: 'groups.actions',
  actionItems: 'groups.actionItems',
  projects: 'groups.projects',
  codingSessions: 'groups.codingSessions',
  initiatives: 'groups.initiatives',
} as const satisfies Record<SearchGroup, ParseKeys<'search'>>
