// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Connecting external accounts (#35, docs/integrations.md): Elysium is the
// OAuth *client*, holding the user's grant to GitHub/Google/Microsoft and
// silently refreshing it. JSON (an OAuth-client flow, not the protobuf control
// plane). Providers are configured by an admin; a member connects their own
// accounts, and may connect several of the same provider.

// The provider a linked account belongs to. Kept as string identifiers matching
// the server, mapped to display metadata at the UI layer.
export type OauthProvider = {
  provider: string
  display_name: string
  // Whether an admin has wired this provider's OAuth app credentials. Only
  // configured providers can be connected.
  configured: boolean
}

export type ConnectedAccount = {
  id: string
  provider: string
  external_username: string
  external_email: string
  avatar_url: string
  connected_at: string
  scopes: string[]
  // True when the stored grant can no longer be refreshed and the user must
  // re-consent. Optional so the card degrades gracefully if the backend does
  // not (yet) report health.
  needs_reconsent?: boolean
}

export function listOauthProviders(): Promise<OauthProvider[]> {
  return apiClient.get('api/v1/oauth/providers').json<OauthProvider[]>()
}

export function listConnectedAccounts(): Promise<ConnectedAccount[]> {
  return apiClient.get('api/v1/me/connected-accounts').json<ConnectedAccount[]>()
}

// Returns the provider's authorization URL to navigate to. Elysium owns the
// `state`/PKCE and handles the callback, redirecting back to the settings route
// with `?connected=<provider>` or `?oauth_error=<reason>`.
export function connectProvider(provider: string): Promise<{ authorize_url: string }> {
  return apiClient.post(`api/v1/oauth/connect/${provider}`).json<{ authorize_url: string }>()
}

// Revokes the grant and forgets the account.
export function disconnectAccount(accountId: string): Promise<Response> {
  return apiClient.delete(`api/v1/me/connected-accounts/${accountId}`)
}
