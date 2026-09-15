// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const NAME_MAX_CHARACTERS = 120
const DESCRIPTION_MAX_CHARACTERS = 2000

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
