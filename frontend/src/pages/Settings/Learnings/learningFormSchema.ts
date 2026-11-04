// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Misc
import { TEXT_MAX_CHARACTERS, TITLE_MAX_CHARACTERS } from './learningPresentation'

// Built per render with `t` so validation messages are already translated. Mirrors the API's
// bounds, which it checks after trimming the same way: the takeaway on one line, the body and
// evidence trimmed and never blank. The category is any key the form offers; the workspace adds
// its own, so the API alone knows which keys exist.
export function createLearningFormSchema(t: TFunction<'learnings'>) {
  const text = (requiredMessage: string) => z
    .string()
    .trim()
    .min(1, { error: requiredMessage })
    .max(TEXT_MAX_CHARACTERS, { error: t('form.errors.textTooLong') })

  return z.object({
    title: z
      .string()
      .transform((title) => title.split(/\s+/u).filter(Boolean).join(' '))
      .pipe(z
        .string()
        .min(1, { error: t('form.errors.titleRequired') })
        .max(TITLE_MAX_CHARACTERS, { error: t('form.errors.titleTooLong') })),
    category: z.string().min(1, { error: t('form.errors.categoryRequired') }),
    body: text(t('form.errors.bodyRequired')),
    evidence: text(t('form.errors.evidenceRequired')),
  })
}

export type LearningFormInput = z.input<ReturnType<typeof createLearningFormSchema>>
export type LearningFormValues = z.output<ReturnType<typeof createLearningFormSchema>>
