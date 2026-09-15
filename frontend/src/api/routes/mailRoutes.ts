// Copyright © 2026 Jalapeno Labs

// Misc
import { API_BASE_PATH } from '../../constants'
import { apiClient } from '../index'

// Mirrors `MailAccountKind` in api/src/models/mail_account.rs.
export type MailAccountKind = 'gmail' | 'outlook' | 'self-hosted'

// Gmail and Outlook connect through the OAuth broker; self-hosted mailboxes are created.
export type OAuthMailAccountKind = Exclude<MailAccountKind, 'self-hosted'>

// The API never returns the credential. `lastError` is null when the latest check
// passed or none has run; `lastCheckedAt` tells the two apart.
export type MailAccount = {
  id: string
  kind: MailAccountKind
  address: string
  displayName: string
  isActive: boolean
  lastCheckedAt: string | null
  lastError: string | null
  createdAt: string
  updatedAt: string
}

export type MailCapabilities = {
  brokerConfigured: boolean
  // Why a configured broker could not be asked which providers it offers.
  brokerError: string | null
  oauthKinds: OAuthMailAccountKind[]
  selfHosted: boolean
}

type GetMailCapabilitiesResponse = {
  capabilities: MailCapabilities
}

export function getMailCapabilities() {
  return apiClient
    .get('v1/mail/capabilities')
    .json<GetMailCapabilitiesResponse>()
}

type ListMailAccountsResponse = {
  accounts: MailAccount[]
}

export function listMailAccounts() {
  return apiClient
    .get('v1/mail/accounts')
    .json<ListMailAccountsResponse>()
}

type CreateMailboxRequest = {
  localPart: string
  domain: string
  displayName?: string
}

type MailAccountResponse = {
  account: MailAccount
}

export function createMailbox(body: CreateMailboxRequest) {
  return apiClient
    .post('v1/mail/accounts', { json: body })
    .json<MailAccountResponse>()
}

// Omitted fields are left unchanged.
type UpdateMailAccountRequest = {
  displayName?: string
  isActive?: boolean
}

export function updateMailAccount(accountId: string, body: UpdateMailAccountRequest) {
  return apiClient
    .patch(`v1/mail/accounts/${accountId}`, { json: body })
    .json<MailAccountResponse>()
}

export function deleteMailAccount(accountId: string) {
  return apiClient
    .delete(`v1/mail/accounts/${accountId}`)
}

// Answers with the account either way: a failed check is recorded in `lastError`.
export function testMailAccount(accountId: string) {
  return apiClient
    .post(`v1/mail/accounts/${accountId}/test`)
    .json<MailAccountResponse>()
}

type SendTestMessageResponse = {
  sentTo: string
}

export function sendMailTestMessage(accountId: string) {
  return apiClient
    .post(`v1/mail/accounts/${accountId}/test-message`)
    .json<SendTestMessageResponse>()
}

// Not a fetch: the browser navigates here and follows the API's redirect to the broker.
export function getOAuthStartHref(kind: OAuthMailAccountKind) {
  return `${API_BASE_PATH}/v1/mail/oauth/${kind}/start`
}
