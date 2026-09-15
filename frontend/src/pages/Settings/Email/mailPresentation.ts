// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'
import type { MailAccount, MailAccountKind, OAuthMailAccountKind } from '../../../api/routes/mailRoutes'

// User interface
import { LuServer } from 'react-icons/lu'
import { PiMicrosoftOutlookLogo } from 'react-icons/pi'
import { SiGmail } from 'react-icons/si'

export type MailAccountHealth = 'working' | 'failing' | 'unchecked' | 'inactive'

export const OAUTH_MAIL_ACCOUNT_KINDS = [ 'gmail', 'outlook' ] as const satisfies readonly OAuthMailAccountKind[]

export const mailAccountKindLabelKeys = {
  'gmail': 'providers.gmail',
  'outlook': 'providers.outlook',
  'self-hosted': 'providers.self-hosted',
} as const satisfies Record<MailAccountKind, ParseKeys<'email'>>

export const mailAccountKindIcons = {
  'gmail': SiGmail,
  'outlook': PiMicrosoftOutlookLogo,
  'self-hosted': LuServer,
} as const satisfies Record<MailAccountKind, IconType>

export const mailAccountHealthLabelKeys = {
  working: 'status.working',
  failing: 'status.failing',
  unchecked: 'status.unchecked',
  inactive: 'status.inactive',
} as const satisfies Record<MailAccountHealth, ParseKeys<'email'>>

export const mailAccountHealthChipColors = {
  working: 'success',
  failing: 'danger',
  unchecked: 'default',
  inactive: 'default',
} as const satisfies Record<MailAccountHealth, 'success' | 'danger' | 'default'>

// Inactive outranks the last check: an inactive mailbox is not used, so how it last
// fared says nothing about now.
export function getMailAccountHealth(account: MailAccount): MailAccountHealth {
  if (!account.isActive) {
    return 'inactive'
  }
  if (account.lastError) {
    return 'failing'
  }
  if (account.lastCheckedAt) {
    return 'working'
  }
  return 'unchecked'
}

// Codes the API's OAuth callback puts in `mailError`. Anything else is shown generically.
export const oauthErrorMessageKeys = {
  access_denied: 'oauthErrors.access_denied',
  state_mismatch: 'oauthErrors.state_mismatch',
  flow_expired: 'oauthErrors.flow_expired',
  broker_unavailable: 'oauthErrors.broker_unavailable',
  broker_refused: 'oauthErrors.broker_refused',
  token_exchange_failed: 'oauthErrors.token_exchange_failed',
  no_refresh_token: 'oauthErrors.no_refresh_token',
  no_address: 'oauthErrors.no_address',
  provider_unavailable: 'oauthErrors.provider_unavailable',
  provider_error: 'oauthErrors.provider_error',
  address_in_use: 'oauthErrors.address_in_use',
  invalid_request: 'oauthErrors.invalid_request',
  internal_error: 'oauthErrors.internal_error',
} as const satisfies Record<string, ParseKeys<'email'>>
