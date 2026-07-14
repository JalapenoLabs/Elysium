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
