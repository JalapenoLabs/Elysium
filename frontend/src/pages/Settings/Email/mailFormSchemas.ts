// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's checks in api/src/routes/v1/mail/create_mailbox.rs, so most mistakes
// are caught before a round trip.
const LOCAL_PART_MAX_CHARACTERS = 64
const DOMAIN_MAX_CHARACTERS = 253
const DISPLAY_NAME_MAX_CHARACTERS = 120
const LOCAL_PART_PATTERN = /^[a-z0-9_+-]+(\.[a-z0-9_+-]+)*$/i
const DOMAIN_PATTERN = /^([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/i

function displayNameField(t: TFunction<'email'>) {
  return z
    .string()
    .trim()
    .max(DISPLAY_NAME_MAX_CHARACTERS, { error: t('createForm.errors.displayNameTooLong') })
}

// Built per render with `t` so validation messages are already translated.
export function createMailboxFormSchema(t: TFunction<'email'>) {
  return z.object({
    localPart: z
      .string()
      .trim()
      .min(1, { error: t('createForm.errors.localPartRequired') })
      .max(LOCAL_PART_MAX_CHARACTERS, { error: t('createForm.errors.localPartInvalid') })
      .regex(LOCAL_PART_PATTERN, { error: t('createForm.errors.localPartInvalid') }),
    domain: z
      .string()
      .trim()
      .max(DOMAIN_MAX_CHARACTERS, { error: t('createForm.errors.domainInvalid') })
      .regex(DOMAIN_PATTERN, { error: t('createForm.errors.domainInvalid') }),
    displayName: displayNameField(t),
  })
}

export type CreateMailboxFormValues = z.infer<ReturnType<typeof createMailboxFormSchema>>

// Mirrors api/src/routes/v1/mail/set_up_server.rs.
const BOOTSTRAP_PASSWORD_MAX_CHARACTERS = 256

export function createSetUpMailServerFormSchema(t: TFunction<'email'>) {
  return z.object({
    domain: z
      .string()
      .trim()
      .max(DOMAIN_MAX_CHARACTERS, { error: t('setupForm.errors.domainInvalid') })
      .regex(DOMAIN_PATTERN, { error: t('setupForm.errors.domainInvalid') }),
    bootstrapPassword: z
      .string()
      .trim()
      .min(1, { error: t('setupForm.errors.passwordRequired') })
      .max(BOOTSTRAP_PASSWORD_MAX_CHARACTERS, { error: t('setupForm.errors.passwordRejected') }),
  })
}

export type SetUpMailServerFormValues = z.infer<ReturnType<typeof createSetUpMailServerFormSchema>>

export function createSenderNameFormSchema(t: TFunction<'email'>) {
  return z.object({
    displayName: displayNameField(t),
  })
}

export type SenderNameFormValues = z.infer<ReturnType<typeof createSenderNameFormSchema>>
