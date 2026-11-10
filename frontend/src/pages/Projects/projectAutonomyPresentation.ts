// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { ProjectAutonomy } from '../../api/routes/projectRoutes'

// Each level of a project's trust dial in words: its name, and what of the Project Manager's work
// waits for the person at it. See docs/autonomy.md, The dial.

export const autonomyLabelKeys = {
  approve_every_batch: 'autonomy.levels.approve_every_batch',
  approve_found_work: 'autonomy.levels.approve_found_work',
  full_autonomy: 'autonomy.levels.full_autonomy',
} as const satisfies Record<ProjectAutonomy, ParseKeys<'projects'>>

export const autonomyDescriptionKeys = {
  approve_every_batch: 'autonomy.descriptions.approve_every_batch',
  approve_found_work: 'autonomy.descriptions.approve_found_work',
  full_autonomy: 'autonomy.descriptions.full_autonomy',
} as const satisfies Record<ProjectAutonomy, ParseKeys<'projects'>>
