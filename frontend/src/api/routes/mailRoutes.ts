// Copyright © 2026 Jalapeno Labs

// Misc
import { API_BASE_PATH, MAIL_DNS_CHECK_TIMEOUT_MS } from '../../constants'
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
  // The mail domain of a self-hosted mailbox; null for OAuth accounts.
  mailDomainId: string | null
}

export type MailCapabilities = {
  brokerConfigured: boolean
  // Why a configured broker could not be asked which providers it offers.
  brokerError: string | null
  oauthKinds: OAuthMailAccountKind[]
}

type GetMailCapabilitiesResponse = {
  capabilities: MailCapabilities
}

export function getMailCapabilities() {
  return apiClient
    .get('v1/mail/capabilities')
    .json<GetMailCapabilitiesResponse>()
}

// Mirrors `MailServerState` and `Step` in api/src/mail/hosting.rs.
export type MailServerState = 'not-created' | 'creating' | 'failed' | 'ready' | 'unreachable'
export type MailServerStep = 'preparing' | 'pulling-image' | 'starting' | 'configuring' | 'restarting' | 'adding-domain'

export type MailServer = {
  state: MailServerState
  // The name the server answers as, once it exists.
  hostname: string | null
  // The step in progress while `creating`.
  step: MailServerStep | null
  // Why creation failed, or why the server is unreachable.
  error: string | null
}

type MailServerResponse = {
  server: MailServer
}

export function getMailServer() {
  return apiClient
    .get('v1/mail/server')
    .json<MailServerResponse>()
}

type CreateMailServerRequest = {
  hostname: string
  domain: string
}

// Answers at once with the server `creating`; `mailServer.updated` events follow.
export function createMailServer(body: CreateMailServerRequest) {
  return apiClient
    .post('v1/mail/server', { json: body })
    .json<MailServerResponse>()
}

export type MailDomain = {
  id: string
  name: string
  // The domain the server was created with, which cannot be removed.
  isDefault: boolean
  createdAt: string
}

type ListMailDomainsResponse = {
  domains: MailDomain[]
}

export function listMailDomains() {
  return apiClient
    .get('v1/mail/domains')
    .json<ListMailDomainsResponse>()
}

type CreateMailDomainResponse = {
  domain: MailDomain
}

export function createMailDomain(name: string) {
  return apiClient
    .post('v1/mail/domains', { json: { name }})
    .json<CreateMailDomainResponse>()
}

export function deleteMailDomain(domainId: string) {
  return apiClient
    .delete(`v1/mail/domains/${domainId}`)
}

// Mirrors `CheckedRecord` in api/src/mail/dns.rs.
export type DnsRecordPurpose = 'mx' | 'spf' | 'dkim' | 'dmarc'
export type DnsRecordStatus = 'published' | 'different' | 'missing' | 'unverified'

export type CheckedDnsRecord = {
  purpose: DnsRecordPurpose
  recordType: 'MX' | 'TXT'
  name: string
  value: string
  status: DnsRecordStatus
  // What public DNS serves for the same purpose.
  found: string[]
  error: string | null
}

type CheckMailDomainDnsResponse = {
  records: CheckedDnsRecord[]
  // Every record Stalwart recommends, including the optional ones the check leaves out.
  zoneFile: string
}

export function checkMailDomainDns(domainId: string) {
  return apiClient
    .get(`v1/mail/domains/${domainId}/dns`, { timeout: MAIL_DNS_CHECK_TIMEOUT_MS })
    .json<CheckMailDomainDnsResponse>()
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
  domainId: string
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
