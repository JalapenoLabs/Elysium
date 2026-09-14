// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const NAME_MAX_CHARACTERS = 120
const DESCRIPTION_MAX_CHARACTERS = 2000
const URL_MAX_CHARACTERS = 2048
const SECRET_MAX_BYTES = 1024

export type SatelliteFormMode = 'create' | 'edit'

// Built per render with `t` so validation messages are already translated.
export function createSatelliteFormSchema(t: TFunction<'satellites'>, mode: SatelliteFormMode) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    description: z
      .string()
      .max(DESCRIPTION_MAX_CHARACTERS, { error: t('form.errors.descriptionTooLong') }),
    url: z
      .string()
      .trim()
      .max(URL_MAX_CHARACTERS, { error: t('form.errors.urlInvalid') })
      .refine(
        (value) => URL.canParse(value) && /^https?:\/\//.test(value),
        { error: t('form.errors.urlInvalid') },
      ),
    secret: z
      .string()
      .refine(
        (value) => new TextEncoder().encode(value).length <= SECRET_MAX_BYTES,
        { error: t('form.errors.secretTooLong') },
      )
      .refine(
        // Editing keeps the stored secret when this is left blank.
        (value) => mode === 'edit' || value.trim().length > 0,
        { error: t('form.errors.secretRequired') },
      ),
    isActive: z.boolean(),
  })
}

export type SatelliteFormValues = z.infer<ReturnType<typeof createSatelliteFormSchema>>
