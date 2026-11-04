// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// The bounds the API holds a category to (api/src/routes/v1/learning_categories/).
export const LABEL_MAX_CHARACTERS = 60
export const KEY_MAX_CHARACTERS = 50
export const DESCRIPTION_MAX_CHARACTERS = 500

// A lowercase letter, then lowercase letters, digits, or underscores, as
// `learning_categories.key` checks.
const KEY_PATTERN = /^[a-z][a-z0-9_]*$/u

// The key a label suggests while the key has not been typed by hand: its words, lowercased
// and joined by underscores, with anything a key cannot hold dropped, so "EDA tools" suggests
// `eda_tools`. Accents fall away rather than split a word, and leading digits are dropped,
// since a key starts with a letter.
export function keyFromLabel(label: string) {
  const words = label
    .normalize('NFKD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/gu, ' ')
    .trim()
    .split(' ')
    .filter(Boolean)
  return words
    .join('_')
    .replace(/^[0-9_]+/u, '')
    .slice(0, KEY_MAX_CHARACTERS)
    .replace(/_+$/u, '')
}

// Built per render with `t` so validation messages are already translated. Mirrors the API's
// bounds, which it checks after trimming the same way.
export function createLearningCategoryFormSchema(t: TFunction<'learningCategories'>) {
  return z.object({
    label: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.labelRequired') })
      .max(LABEL_MAX_CHARACTERS, { error: t('form.errors.labelTooLong') }),
    key: z
      .string()
      .trim()
      .max(KEY_MAX_CHARACTERS, { error: t('form.errors.keyTooLong') })
      .regex(KEY_PATTERN, { error: t('form.errors.keyInvalid') }),
    description: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.descriptionRequired') })
      .max(DESCRIPTION_MAX_CHARACTERS, { error: t('form.errors.descriptionTooLong') }),
  })
}

export type LearningCategoryFormInput = z.input<ReturnType<typeof createLearningCategoryFormSchema>>
export type LearningCategoryFormValues = z.output<ReturnType<typeof createLearningCategoryFormSchema>>
