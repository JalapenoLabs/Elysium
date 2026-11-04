// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'
import type {
  MailAccount,
  MailAccountKind,
  MailServer,
  MailServerState,
  OAuthMailAccountKind,
} from '../../../api/routes/mailRoutes'

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

// Why the mail server cannot take a new domain or mailbox, by its state; null when it can.
export const mailServerBlockerKeys = {
  'not-created': 'availability.mailServerMissing',
  'creating': 'availability.mailServerMissing',
  'failed': 'availability.mailServerMissing',
  'ready': null,
  'unreachable': 'server.unreachable.title',
} as const satisfies Record<MailServerState, ParseKeys<'email'> | null>

// Why a mailbox cannot be created on the mail server yet, as a translation key: the server is
// not ready, or it has no domain for the address. Null when it can.
export function getMailboxBlockerKey(server: MailServer, hasDomains: boolean): ParseKeys<'email'> | null {
  const serverBlocker = mailServerBlockerKeys[server.state]
  if (serverBlocker) {
    return serverBlocker
  }
  if (!hasDomains) {
    return 'availability.mailDomainMissing'
  }
  return null
}
