// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's limits in api/src/routes/v1/studio_items/create_studio_item.rs.
export const STUDIO_TITLE_MAX_CHARACTERS = 200
export const STUDIO_PROMPT_MAX_CHARACTERS = 100_000

// Built per render with `t` so validation messages are already translated.
export function createStudioItemFormSchema(t: TFunction<'studio'>) {
  return z.object({
    prompt: z
      .string()
      .max(STUDIO_PROMPT_MAX_CHARACTERS)
      .refine((value) => value.trim().length > 0, { error: t('create.errors.promptRequired') }),
    // Optional: a blank title is left to the API, which names the item after the prompt.
    title: z
      .string()
      .trim()
      .max(STUDIO_TITLE_MAX_CHARACTERS, {
        error: t('create.errors.titleTooLong', { max: STUDIO_TITLE_MAX_CHARACTERS }),
      }),
    // Empty for no project.
    projectId: z.string(),
    storageLocationId: z
      .string()
      .min(1, { error: t('create.errors.locationRequired') }),
    satelliteId: z
      .string()
      .min(1, { error: t('create.errors.satelliteRequired') }),
  })
}

export type StudioItemFormValues = z.infer<ReturnType<typeof createStudioItemFormSchema>>
