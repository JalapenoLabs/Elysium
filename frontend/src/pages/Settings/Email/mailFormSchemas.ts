// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's checks in api/src/routes/v1/mail/, so most mistakes are caught before
// a round trip.
const LOCAL_PART_MAX_CHARACTERS = 64
const DOMAIN_MAX_CHARACTERS = 253
export const DISPLAY_NAME_MAX_CHARACTERS = 120
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
    domainId: z
      .string()
      .min(1, { error: t('createForm.errors.domainRequired') }),
    displayName: displayNameField(t),
  })
}

export type CreateMailboxFormValues = z.infer<ReturnType<typeof createMailboxFormSchema>>

// A domain name, the same rule the API applies to domains and the server's hostname.
function domainNameField(message: string) {
  return z
    .string()
    .trim()
    .max(DOMAIN_MAX_CHARACTERS, { error: message })
    .regex(DOMAIN_PATTERN, { error: message })
}

export function createMailServerFormSchema(t: TFunction<'email'>) {
  return z.object({
    domain: domainNameField(t('serverForm.errors.domainInvalid')),
    hostname: domainNameField(t('serverForm.errors.hostnameInvalid')),
  })
}

export type MailServerFormValues = z.infer<ReturnType<typeof createMailServerFormSchema>>

export function createMailDomainFormSchema(t: TFunction<'email'>) {
  return z.object({
    name: domainNameField(t('domainForm.errors.nameInvalid')),
  })
}

export type MailDomainFormValues = z.infer<ReturnType<typeof createMailDomainFormSchema>>

