// Copyright © 2026 Jalapeno Labs

import type { ManualStatusKind, User } from '../../proto/generated/user_pb'

// Lib
import { create } from '@bufbuild/protobuf'

// Redux
import { dispatch } from '../../store'
import { currentUserLoaded, toCurrentUser } from '../../store/currentUserSlice'

// Misc
import { apiClient, getProto, sendProto } from '../index'
import {
  SetStatusRequestSchema,
  UpdateProfileRequestSchema,
  UserSchema,
} from '../../proto/generated/user_pb'

// Every profile response also lands in the `currentUser` slice (as its
// serializable projection), so the chrome that renders the signed-in user stays
// current without each consumer fetching for itself.
function recordCurrentUser(user: User): User {
  dispatch(currentUserLoaded(toCurrentUser(user)))
  return user
}

// The authenticated user's full profile, including derived presence and any
// active manual-status override. Protected: 401 without a session cookie.
export async function getMyProfile(): Promise<User> {
  const user = await getProto('api/v1/users/me', UserSchema)
  return recordCurrentUser(user)
}

type UpdateMyProfileFields = {
  givenName: string
  familyName: string
  // When blank the server falls back to `givenName`, so callers can leave the
  // display name optional.
  displayName: string
  bio: string
  // Optional partial: omit to leave the picture unchanged, '' to clear it.
  avatarUrl?: string
}

export async function updateMyProfile(fields: UpdateMyProfileFields): Promise<User> {
  const user = await sendProto('api/v1/users/me', {
    method: 'patch',
    requestSchema: UpdateProfileRequestSchema,
    request: create(UpdateProfileRequestSchema, fields),
    responseSchema: UserSchema,
  })
  return recordCurrentUser(user)
}

// Sets or clears the manual status override. `kind` of UNSPECIFIED clears it;
// `text` only applies to CUSTOM; `durationSeconds` of 0 uses the server's 24h
// default. Derived online/away/offline presence is never set by the client.
// TOTP enrollment for the signed-in account: begin returns the provisioning
// URI to render as a QR, confirm stores it once the code verifies. Powers both
// the require-2FA gate and the Security Center's add-authenticator action.
export function beginTotpEnrollment(): Promise<{ provisioning_uri: string }> {
  return apiClient
    .post('api/v1/me/totp/begin')
    .json<{ provisioning_uri: string }>()
}

export function confirmTotpEnrollment(code: string): Promise<Response> {
  return apiClient.post('api/v1/me/totp/confirm', {
    json: { code: code.trim() },
  })
}

export type LoginEvent = {
  method: string
  outcome: string
  ip: string
  user_agent: string
  created_at: string
}

// The signed-in account's recent sign-in attempts, most recent first (its own
// only). Feeds the Security Center's account-access view.
export function getLoginHistory(): Promise<{ events: LoginEvent[] }> {
  return apiClient
    .get('api/v1/me/login-history')
    .json<{ events: LoginEvent[] }>()
}

export type CredentialsSummary = {
  passkeys: { id: string, created_at: string, last_used_at: string }[]
  password: { enrolled: boolean }
  totp: { enrolled: boolean }
}

// Which credentials the account has enrolled: its passkeys (with added and
// last-used times), whether a password is set, and whether an authenticator is
// enrolled. Backs the Security Center's credential views.
export function getCredentials(): Promise<CredentialsSummary> {
  return apiClient
    .get('api/v1/me/credentials')
    .json<CredentialsSummary>()
}

export type ActiveSession = {
  id: string
  user_agent: string
  ip: string
  created_at: string
  last_seen_at: string
  expires_at: string
  current: boolean
}

// Every live session for the account: where it signed in, its device, and when
// it was last active. `current` marks this browser's session.
export function getActiveSessions(): Promise<{ sessions: ActiveSession[] }> {
  return apiClient
    .get('api/v1/me/sessions')
    .json<{ sessions: ActiveSession[] }>()
}

// Revokes one session by id. Revoking the current session clears the cookie
// server-side, which signs this browser out.
export function revokeSession(sessionId: string): Promise<Response> {
  return apiClient.delete(`api/v1/me/sessions/${sessionId}`)
}

// Signs out every OTHER session, leaving this one active.
export function revokeOtherSessions(): Promise<Response> {
  return apiClient.post('api/v1/me/sessions/revoke-others')
}

export async function setMyStatus(
  kind: ManualStatusKind,
  text: string = '',
  durationSeconds: bigint = 0n,
): Promise<User> {
  const user = await sendProto('api/v1/users/me/status', {
    method: 'put',
    requestSchema: SetStatusRequestSchema,
    request: create(SetStatusRequestSchema, { kind, text, durationSeconds }),
    responseSchema: UserSchema,
  })
  return recordCurrentUser(user)
}
