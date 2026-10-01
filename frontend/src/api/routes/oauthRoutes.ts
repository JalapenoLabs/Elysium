// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// OAuth for MCP clients such as Claude Code and Codex. Hydra hands the sign-in and the consent
// to the web app with a challenge, and these answer it as the person signed in here. See
// docs/mcp.md.

// Mirrors `ClientResponse` in api/src/routes/v1/oauth/mod.rs. A client registers itself, so its
// name and website prove nothing and may be empty; `redirectHosts`, where approving sends the
// code, is the part a person can check.
export type OAuthClient = {
  id: string
  name: string
  uri: string
  redirectHosts: string[]
  createdAt: string | null
}

// Where the browser goes next: back to Hydra, which carries on to the client.
type RedirectResponse = {
  redirectTo: string
}

export function acceptOAuthLogin(challenge: string) {
  return apiClient
    .post('v1/oauth/login/accept', { json: { challenge }})
    .json<RedirectResponse>()
}

// What a client asks for. `scopes` is empty when it asked for nothing a person can grant.
// `alreadyApproved` means the person connected it for these scopes before; the page then accepts
// at once, through the same POST as pressing Allow.
export type ConsentResponse = {
  client: OAuthClient
  scopes: string[]
  alreadyApproved: boolean
}

export function getOAuthConsent(challenge: string) {
  return apiClient
    .get('v1/oauth/consent', { searchParams: { challenge }})
    .json<ConsentResponse>()
}

export function acceptOAuthConsent(challenge: string) {
  return apiClient
    .post('v1/oauth/consent/accept', { json: { challenge }})
    .json<RedirectResponse>()
}

export function rejectOAuthConsent(challenge: string) {
  return apiClient
    .post('v1/oauth/consent/reject', { json: { challenge }})
    .json<RedirectResponse>()
}

// A client the signed-in person connected, with every scope it holds.
export type OAuthGrant = {
  client: OAuthClient
  scopes: string[]
  grantedAt: string | null
}

type ListGrantsResponse = {
  grants: OAuthGrant[]
}

export function listOAuthGrants() {
  return apiClient
    .get('v1/oauth/grants')
    .json<ListGrantsResponse>()
}

// Every token the client holds for this person stops working.
export function revokeOAuthGrant(clientId: string) {
  return apiClient
    .delete(`v1/oauth/grants/${encodeURIComponent(clientId)}`)
}

type ListClientsResponse = {
  clients: OAuthClient[]
}

// Every registered client, for admins.
export function listOAuthClients() {
  return apiClient
    .get('v1/oauth/clients')
    .json<ListClientsResponse>()
}

// Removes the client for everyone who connected it, for admins.
export function deleteOAuthClient(clientId: string) {
  return apiClient
    .delete(`v1/oauth/clients/${encodeURIComponent(clientId)}`)
}
