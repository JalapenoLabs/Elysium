// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { ZonedDateTime } from '@internationalized/date'
import { z } from 'zod'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const NAME_MAX_CHARACTERS = 120
const DESCRIPTION_MAX_CHARACTERS = 2000
const SECRET_TOKEN_MAX_BYTES = 16 * 1024

export type LlmFormMode = 'create' | 'edit'

// Built per render with `t` so validation messages are already translated.
export function createLlmFormSchema(t: TFunction<'llms'>, mode: LlmFormMode) {
  const secretToken = z
    .string()
    .refine(
      (value) => new TextEncoder().encode(value).length <= SECRET_TOKEN_MAX_BYTES,
      { error: t('form.errors.secretTokenTooLong') },
    )
    .refine(
      // Editing keeps the stored token when this is left blank.
      (value) => mode === 'edit' || value.trim().length > 0,
      { error: t('form.errors.secretTokenRequired') },
    )

  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    description: z
      .string()
      .max(DESCRIPTION_MAX_CHARACTERS, { error: t('form.errors.descriptionTooLong') }),
    secretToken,
    priority: z.number().int(),
    isActive: z.boolean(),
    // A zoned value in the viewer's time zone; converted to a UTC instant on submit.
    expiresAt: z.instanceof(ZonedDateTime).nullable(),
  })
}

export type LlmFormValues = z.infer<ReturnType<typeof createLlmFormSchema>>
