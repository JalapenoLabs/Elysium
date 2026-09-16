// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { EnvironmentVariable } from '../../../api/routes/environmentRoutes'

// Utility
import { z } from 'zod'

// Misc
import {
  DESCRIPTION_MAX_CHARACTERS,
  getKeyRefusal,
  KEY_MAX_CHARACTERS,
  keyRefusalMessageKeys,
  VALUE_MAX_BYTES,
} from './environmentPresentation'

// Built per render with `t` so validation messages are already translated. `stored` is the
// variable being edited, or null when adding one.
export function createEnvironmentFormSchema(
  t: TFunction<'environment'>,
  stored: EnvironmentVariable | null,
) {
  return z.object({
    key: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.keyRequired') })
      .max(KEY_MAX_CHARACTERS, { error: t('form.errors.keyTooLong') }),
    // Never trimmed: spaces and line breaks can be part of a value, and the API keeps them.
    value: z.string(),
    isSecret: z.boolean(),
    description: z
      .string()
      .trim()
      .max(DESCRIPTION_MAX_CHARACTERS, { error: t('form.errors.descriptionTooLong') }),
  }).superRefine((values, context) => {
    function fail(path: string, message: string) {
      context.addIssue({ code: 'custom', path: [ path ], message })
    }

    const refusal = values.key && getKeyRefusal(values.key)
    if (refusal) {
      fail('key', t(keyRefusalMessageKeys[refusal]))
    }

    // The API bounds the value in bytes, and one character can take up to four.
    if (new TextEncoder().encode(values.value).length > VALUE_MAX_BYTES) {
      fail('value', t('form.errors.valueTooLong'))
      return
    }

    if (values.value) {
      return
    }

    // A blank value is a real, empty value for a visible variable, with one exception: a
    // secret is only revealed by sending its value again, as the API enforces.
    const isRevealingSecret = stored?.isSecret && !values.isSecret
    if (isRevealingSecret) {
      fail('value', t('form.valueReveal'))
      return
    }

    // A secret needs a value, unless it is already a secret and a blank field keeps it.
    const keepsStoredSecret = stored?.isSecret && values.isSecret
    if (values.isSecret && !keepsStoredSecret) {
      fail('value', t('form.errors.valueRequired'))
    }
  })
}

export type EnvironmentFormInput = z.input<ReturnType<typeof createEnvironmentFormSchema>>
export type EnvironmentFormValues = z.output<ReturnType<typeof createEnvironmentFormSchema>>
