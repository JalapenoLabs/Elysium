// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's checks in api/src/routes/v1/browser_sessions/create_browser_session.rs.
const NAME_MAX_CHARACTERS = 100
const START_URL_MAX_CHARACTERS = 2048

// A sign-in page is https, named by its host, never an address.
export function isHttpsHostUrl(value: string) {
  if (!URL.canParse(value)) {
    return false
  }
  const url = new URL(value)
  const isAddress = /^[\d.]+$/.test(url.hostname) || url.hostname.startsWith('[')
  return url.protocol === 'https:' && Boolean(url.hostname) && !isAddress
}

// Built per render with `t` so validation messages are already translated.
export function createBrowserFormSchema(t: TFunction<'browsers'>) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    startUrl: z
      .string()
      .trim()
      .max(START_URL_MAX_CHARACTERS, { error: t('form.errors.startUrlInvalid') })
      .refine(isHttpsHostUrl, { error: t('form.errors.startUrlInvalid') }),
  })
}

export type BrowserFormValues = z.infer<ReturnType<typeof createBrowserFormSchema>>
