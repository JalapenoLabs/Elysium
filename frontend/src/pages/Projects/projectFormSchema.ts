// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Misc
import { PROJECT_COVER_FITS } from '../../api/routes/projectRoutes'

// Mirrors the API's limits so most mistakes are caught before a round trip.
export const NAME_MAX_CHARACTERS = 120
export const DESCRIPTION_MAX_CHARACTERS = 2000

// Built per render with `t` so validation messages are already translated.
export function createProjectFormSchema(t: TFunction<'projects'>) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    description: z
      .string()
      .max(DESCRIPTION_MAX_CHARACTERS, { error: t('form.errors.descriptionTooLong') }),
  })
}

export type ProjectFormValues = z.infer<ReturnType<typeof createProjectFormSchema>>

// A project's own page also sets how its cover fits and which GitHub token its sessions start
// with, as the token picker's key (`toGithubKey`).
export function createProjectEditSchema(t: TFunction<'projects'>) {
  return createProjectFormSchema(t).extend({
    coverFit: z.enum(PROJECT_COVER_FITS),
    githubKey: z.string(),
  })
}

export type ProjectEditValues = z.infer<ReturnType<typeof createProjectEditSchema>>
