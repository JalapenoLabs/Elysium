// Copyright © 2026 Jalapeno Labs

import type { Language, ManualStatusKind, User } from '../../proto/generated/user_pb'
import type { SecurityActivity } from '../../proto/generated/security_pb'

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
import { SecurityActivitySchema } from '../../proto/generated/security_pb'

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
  pronouns: string
  webLink: string
  contactPhone: string
  backupEmail: string
  // Preferred UI language; UNSPECIFIED means auto-detect. Always sent so the
  // full-replace PATCH never resets it while other fields are edited.
  language: Language
  // Preferred display timezone as an IANA name; empty means auto-detect. The
  // server validates it and clamps an unknown zone back to empty rather than
  // rejecting the save, so always send a real IANA name.
  timezone: string
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

// Stores the authenticator once its code verifies. `name` labels it so the user
// can tell multiple authenticators apart; the server defaults it to
// "Authenticator" when omitted. The first authenticator becomes the default.
export function confirmTotpEnrollment(code: string, name?: string): Promise<Response> {
  return apiClient.post('api/v1/me/totp/confirm', {
    json: { code: code.trim(), name: name?.trim() || undefined },
  })
}

// Renames one authenticator so multiple are distinguishable.
export function renameTotpMethod(methodId: string, name: string): Promise<Response> {
  return apiClient.patch(`api/v1/me/totp/${methodId}`, { json: { name: name.trim() }})
}

// Makes one authenticator the default; the previous default steps down.
export function setDefaultTotpMethod(methodId: string): Promise<Response> {
  return apiClient.post(`api/v1/me/totp/${methodId}/default`)
}

// Removes one authenticator. The server promotes another to default if needed.
export function deleteTotpMethod(methodId: string): Promise<Response> {
  return apiClient.delete(`api/v1/me/totp/${methodId}`)
}

export type PasskeySummary = {
  id: string
  // May be empty until the passkey is named.
  name: string
  created_at: string
  last_used_at: string
}

export type TotpMethodSummary = {
  id: string
  // Defaults to "Authenticator" server-side until renamed.
  name: string
  created_at: string
  last_used_at: string
  is_default: boolean
}

export type CredentialsSummary = {
  passkeys: PasskeySummary[]
  password: { enrolled: boolean }
  // Zero or more named authenticator apps. "Has 2FA" is `totp_methods.length > 0`.
  totp_methods: TotpMethodSummary[]
}

// Which credentials the account has enrolled: its passkeys (with added and
// last-used times), whether a password is set, and the enrolled authenticator
// apps. Backs the Security Center's credential views.
export function getCredentials(): Promise<CredentialsSummary> {
  return apiClient
    .get('api/v1/me/credentials')
    .json<CredentialsSummary>()
}

// Renames one passkey so multiple devices are distinguishable.
export function renamePasskey(passkeyId: string, name: string): Promise<Response> {
  return apiClient.patch(`api/v1/me/passkeys/${passkeyId}`, { json: { name: name.trim() }})
}

// Removes one passkey. Rejects with 409 when it is the account's only remaining
// way to sign in, so the caller can explain why it can't be removed.
export function deletePasskey(passkeyId: string): Promise<Response> {
  return apiClient.delete(`api/v1/me/passkeys/${passkeyId}`)
}

// The account's security activity in one protobuf response: live sessions and
// recent sign-ins, each carrying best-effort IP geolocation and `Timestamp`
// instants (render via the Timestamp helper). Replaces the former separate
// `/me/sessions` and `/me/login-history` JSON routes.
export function getSecurityActivity(): Promise<SecurityActivity> {
  return getProto('api/v1/me/security-activity', SecurityActivitySchema)
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
