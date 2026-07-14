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
