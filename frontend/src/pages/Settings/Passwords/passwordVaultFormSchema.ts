// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { PasswordVault } from '../../../api/routes/passwordVaultRoutes'

// Utility
import { z } from 'zod'

// Misc
import { PASSWORD_SERVERS } from '../../../api/routes/passwordVaultRoutes'
import { isServerUrl } from './passwordsPresentation'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const CLIENT_ID_MAX_CHARACTERS = 200
const SECRET_MAX_CHARACTERS = 1024

// Built per render with `t` so messages are already translated. `stored` is the vault being
// changed, whose secrets a blank field keeps, or null when connecting one.
export function createPasswordVaultFormSchema(t: TFunction<'passwords'>, stored: PasswordVault | null) {
  return z.object({
    server: z.enum(PASSWORD_SERVERS),
    serverUrl: z.string().trim(),
    clientId: z.string().trim(),
    clientSecret: z.string(),
    masterPassword: z.string(),
  }).superRefine((values, context) => {
    function fail(path: string, message: string) {
      context.addIssue({ code: 'custom', path: [ path ], message })
    }

    if (values.server === 'self-hosted') {
      if (!values.serverUrl) {
        fail('serverUrl', t('form.errors.serverUrlRequired'))
      }
      else if (!isServerUrl(values.serverUrl)) {
        fail('serverUrl', t('form.errors.serverUrlInvalid'))
      }
    }

    if (!values.clientId) {
      fail('clientId', t('form.errors.clientIdRequired'))
    }
    else if (!values.clientId.startsWith('user.')) {
      fail('clientId', t('form.errors.clientIdShape'))
    }
    else if (values.clientId.length > CLIENT_ID_MAX_CHARACTERS) {
      fail('clientId', t('form.errors.tooLong'))
    }

    // A connected vault keeps its stored secrets when a field is left blank.
    if (!values.clientSecret && !stored) {
      fail('clientSecret', t('form.errors.clientSecretRequired'))
    }
    if (!values.masterPassword && !stored) {
      fail('masterPassword', t('form.errors.masterPasswordRequired'))
    }
    if (values.clientSecret.length > SECRET_MAX_CHARACTERS) {
      fail('clientSecret', t('form.errors.tooLong'))
    }
    if (values.masterPassword.length > SECRET_MAX_CHARACTERS) {
      fail('masterPassword', t('form.errors.tooLong'))
    }
  })
}

export type PasswordVaultFormInput = z.input<ReturnType<typeof createPasswordVaultFormSchema>>
export type PasswordVaultFormValues = z.output<ReturnType<typeof createPasswordVaultFormSchema>>
