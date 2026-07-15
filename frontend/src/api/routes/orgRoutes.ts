// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

type CreateOrgResponse = {
  id: string
  name: string
  slug: string
}

// Creates a new organization owned by the active account; the session's active
// org switches to it server-side.
export function createOrg(name: string): Promise<CreateOrgResponse> {
  return apiClient
    .post('api/v1/orgs', { json: { name }})
    .json<CreateOrgResponse>()
}

type UpdateOrgRequest = {
  name?: string
  logo_url?: string
  // White-label branding: `#RRGGBB` accent (server-validated) and a short
  // brand text; empty strings fall back to Elysium defaults.
  accent_color?: string
  wordmark?: string
}

type UpdateOrgResponse = {
  id: string
  name: string
  logo_url: string
  accent_color: string
  wordmark: string
}

// Updates an org's identity (owner/admin only; 403 otherwise). The server fans
// out `org.updated` so every member's chrome refreshes live.
export function updateOrg(orgId: string, body: UpdateOrgRequest): Promise<UpdateOrgResponse> {
  return apiClient
    .patch(`api/v1/orgs/${orgId}`, { json: body })
    .json<UpdateOrgResponse>()
}

export type OrgMember = {
  user_id: string
  email: string
  display_name: string
  avatar_url: string
  role: string
  suspended: boolean
  joined_at: string
}

// The org's roster. Any member may view; mutations below are admin+ and the
// server enforces rank (no editing yourself, owners, or above your own role).
export function listOrgMembers(orgId: string): Promise<{ members: OrgMember[] }> {
  return apiClient
    .get(`api/v1/orgs/${orgId}/members`)
    .json<{ members: OrgMember[] }>()
}

export function updateOrgMemberRole(orgId: string, userId: string, role: string): Promise<Response> {
  return apiClient.patch(`api/v1/orgs/${orgId}/members/${userId}`, {
    json: { role },
  })
}

export function removeOrgMember(orgId: string, userId: string): Promise<Response> {
  return apiClient.delete(`api/v1/orgs/${orgId}/members/${userId}`)
}

// Suspends a member: they keep their membership but lose access until
// reactivated (their live sessions are dropped server-side).
export function suspendOrgMember(orgId: string, userId: string): Promise<Response> {
  return apiClient.post(`api/v1/orgs/${orgId}/members/${userId}/suspend`)
}

export function reactivateOrgMember(orgId: string, userId: string): Promise<Response> {
  return apiClient.post(`api/v1/orgs/${orgId}/members/${userId}/reactivate`)
}

// Hands the org's ownership to another member: they become owner, the caller is
// demoted to admin. Owner-only, irreversible without the new owner's help.
export function transferOrgOwnership(orgId: string, userId: string): Promise<Response> {
  return apiClient.post(`api/v1/orgs/${orgId}/transfer-ownership`, {
    json: { user_id: userId },
  })
}

// Delegated admin capabilities. Owners and admins hold all of these implicitly;
// they can also grant individual capabilities to members, who then gain access
// to just that admin area (the server enforces it; 403 rules are unchanged).
export type OrgCapability = 'manage_members' | 'manage_security' | 'manage_teams' | 'view_audit'

export type OrgGrant = {
  user_id: string
  email: string
  display_name: string
  capability: OrgCapability
}

export function listOrgGrants(orgId: string): Promise<{ grants: OrgGrant[] }> {
  return apiClient
    .get(`api/v1/orgs/${orgId}/grants`)
    .json<{ grants: OrgGrant[] }>()
}

// Replaces a member's full capability set (owner/admin only).
export function setOrgMemberGrants(
  orgId: string,
  userId: string,
  capabilities: OrgCapability[],
): Promise<Response> {
  return apiClient.put(`api/v1/orgs/${orgId}/members/${userId}/grants`, {
    json: { capabilities },
  })
}

export type AuditEntry = {
  action: string
  actor: { email: string, display_name: string }
  target: { email?: string, display_name?: string } | null
  detail: string
  created_at: string
}

// The org's audit trail, most recent first (admin or `view_audit`).
export function getOrgAuditLog(orgId: string): Promise<{ entries: AuditEntry[] }> {
  return apiClient
    .get(`api/v1/orgs/${orgId}/audit-log`)
    .json<{ entries: AuditEntry[] }>()
}

// Which sign-in methods the org permits, whether a second factor is mandatory,
// and how long sessions live. Members view; admins and owners change it.
export type OrgAuthPolicy = {
  allow_passkeys: boolean
  allow_passwords: boolean
  allow_magic_links: boolean
  require_totp: boolean
  session_lifetime_seconds: number
}

export function getOrgAuthPolicy(orgId: string): Promise<OrgAuthPolicy> {
  return apiClient
    .get(`api/v1/orgs/${orgId}/auth-policy`)
    .json<OrgAuthPolicy>()
}

// Accepts any subset. The server rejects (400) a change that would disable
// every sign-in method or set a lifetime outside 5 minutes .. 90 days.
export function updateOrgAuthPolicy(
  orgId: string,
  body: Partial<OrgAuthPolicy>,
): Promise<OrgAuthPolicy> {
  return apiClient
    .patch(`api/v1/orgs/${orgId}/auth-policy`, { json: body })
    .json<OrgAuthPolicy>()
}
