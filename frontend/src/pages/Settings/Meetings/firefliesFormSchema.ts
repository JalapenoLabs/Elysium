// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's limit, so a paste of the wrong thing is caught before a round trip.
const API_KEY_MAX_CHARACTERS = 512

// Built per render with `t` so messages are already translated.
export function createFirefliesFormSchema(t: TFunction<'meetings'>) {
  return z.object({
    apiKey: z.string()
      .trim()
      .min(1, t('form.errors.apiKeyRequired'))
      .max(API_KEY_MAX_CHARACTERS, t('form.errors.apiKeyTooLong')),
    storageLocationId: z.string().min(1, t('form.errors.storageLocationRequired')),
  })
}

export type FirefliesFormInput = z.input<ReturnType<typeof createFirefliesFormSchema>>
export type FirefliesFormValues = z.output<ReturnType<typeof createFirefliesFormSchema>>
