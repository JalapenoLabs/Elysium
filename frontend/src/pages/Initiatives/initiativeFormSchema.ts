// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { CalendarDate } from '@internationalized/date'
import { z } from 'zod'

// Misc
import { INITIATIVE_DESCRIPTION_MAX_CHARACTERS, INITIATIVE_NAME_MAX_CHARACTERS } from './initiativePresentation'

// Built per render with `t` so validation messages are already translated.
export function createInitiativeFormSchema(t: TFunction<'initiatives'>) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(INITIATIVE_NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    description: z
      .string()
      .max(INITIATIVE_DESCRIPTION_MAX_CHARACTERS, { error: t('form.errors.descriptionTooLong') }),
    // A day in the viewer's zone; the target lasts until it ends.
    targetDate: z.instanceof(CalendarDate).nullable(),
    projectIds: z.array(z.string()),
  })
}

export type InitiativeFormInput = z.input<ReturnType<typeof createInitiativeFormSchema>>
export type InitiativeFormValues = z.output<ReturnType<typeof createInitiativeFormSchema>>
