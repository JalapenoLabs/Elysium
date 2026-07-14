// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// One signed-in profile within this browser's session. A session can hold
// several (Google-style); exactly one is active and is what `/api/v1/me`
// answers for.
export type SessionAccount = {
  user_id: string
  email: string
  display_name: string
  avatar_url: string
  is_active: boolean
}

// An org the ACTIVE account belongs to (profiles are org-many).
export type SessionOrg = {
  id: string
  name: string
  slug: string
  role: string
}

type SessionAccountsResponse = {
  accounts: SessionAccount[]
  orgs: SessionOrg[]
  active_org_id: string
}

// Every account signed into this browser session, plus the active account's
// orgs. Backs the profile-badge dropdown (and the future org switcher).
export function listSessionAccounts(): Promise<SessionAccountsResponse> {
  return apiClient
    .get('api/v1/session/accounts')
    .json<SessionAccountsResponse>()
}

// Flips the active account to another one already held by this session. The
// caller re-resolves the session afterward; `/api/v1/me` then answers for the
// newly active account.
export function switchAccount(userId: string): Promise<Response> {
  return apiClient.post('api/v1/session/switch', {
    json: { user_id: userId },
  })
}
