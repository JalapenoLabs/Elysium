// Copyright © 2026 Jalapeno Labs

import type { MeResponse } from '../../proto/generated/auth_pb'
import type { AuthMethodKind } from '../../auth/methods/types'
import type {
  PublicKeyCredentialCreationOptionsJSON,
  PublicKeyCredentialRequestOptionsJSON,
} from '@simplewebauthn/browser'

// Lib
import { startAuthentication, startRegistration } from '@simplewebauthn/browser'

// Redux
import { dispatch } from '../../store'
import { sessionAnonymous } from '../../store/sessionSlice'

// Misc
import { apiClient, getProto } from '../index'
import { MeResponseSchema } from '../../proto/generated/auth_pb'

// The auth ceremony is standardized WebAuthn JSON, not protobuf: it is what
// `navigator.credentials` consumes and produces. The API (our WebAuthn Relying
// Party) wraps the options under `publicKey`; the browser never contacts any
// identity provider directly. See docs/auth-model.md.
type RequestChallenge = { publicKey: PublicKeyCredentialRequestOptionsJSON }
type CreationChallenge = { publicKey: PublicKeyCredentialCreationOptionsJSON }

// Fetches the authenticated principal. Protected: the request carries the session
// cookie (attached automatically) and the API returns 401 without a valid one.
// This doubles as the session probe on load: a 200 means authenticated, a 401
// means anonymous.
export function getMe(): Promise<MeResponse> {
  return getProto('api/v1/me', MeResponseSchema)
}

// First-run check: whether this instance has been claimed by a root user yet.
export function getAuthContext(): Promise<{ claimed: boolean }> {
  return apiClient.get('api/v1/auth/context').json<{ claimed: boolean }>()
}

// Passwordless sign-in with a registered passkey.
//
// Posts the email, runs the native authentication ceremony
// (`navigator.credentials.get`), and posts the assertion back. On success the API
// has verified it and set the session cookie; the caller re-resolves the session.
export async function signInWithPasskey(email: string): Promise<void> {
  const challenge = await apiClient
    .post('api/v1/auth/passkey/authenticate/begin', { json: { email }})
    .json<RequestChallenge>()

  const assertion = await startAuthentication({ optionsJSON: challenge.publicKey })

  await apiClient.post('api/v1/auth/passkey/authenticate/finish', { json: assertion })
}

// Ends the session. The API clears the session cookie server-side. The browser
// holds no token, so there is nothing to discard client-side beyond the local
// Redux state, which this clears in a `finally` so it happens wherever `logout` is
// called, even if the request itself fails.
export async function logout(): Promise<Response> {
  try {
    return await apiClient.post('api/v1/auth/logout')
  }
  finally {
    dispatch(sessionAnonymous())
  }
}

// ///////////////////////////////////////////////////////////// //
//  Method discovery + OPAQUE password + TOTP (raw HTTP only)     //
//  Consumed by the headless hooks under `auth/methods`.          //
// ///////////////////////////////////////////////////////////// //

// The sign-in methods this instance permits for an identifier. Non-enumerating:
// the server answers from org policy, never from which credentials exist, so an
// unknown identifier still gets a normal method list. See docs/auth-model.md.
export function resolveAuthMethods(identifier: string): Promise<{ methods: AuthMethodKind[] }> {
  return apiClient
    .post('api/v1/auth/methods', { json: { identifier }})
    .json<{ methods: AuthMethodKind[] }>()
}

// OPAQUE login step 1: exchange KE1 for the server's KE2. The `ke1`/`ke2` values
// are the base64 messages `@serenity-kit/opaque` produces and consumes.
export function passwordAuthenticateBegin(identifier: string, ke1: string): Promise<{ ke2: string }> {
  return apiClient
    .post('api/v1/auth/password/authenticate/begin', { json: { identifier, ke1 }})
    .json<{ ke2: string }>()
}

// OPAQUE login step 2: submit KE3. Returns the raw response so the caller can tell
// `204` (signed in) from `401 { reason: 'totp_required' }` (needs a second factor);
// `throwHttpErrors: false` keeps the 401 from throwing.
export function passwordAuthenticateFinish(ke3: string): Promise<Response> {
  return apiClient.post('api/v1/auth/password/authenticate/finish', {
    json: { ke3 },
    throwHttpErrors: false,
  })
}

// Completes a password login's TOTP second factor. Returns the raw response so the
// caller can distinguish `204` (signed in) from `401` (wrong code).
export function verifyTotp(code: string): Promise<Response> {
  return apiClient.post('api/v1/auth/totp/verify', {
    json: { code },
    throwHttpErrors: false,
  })
}

// First-run claim, step 1 (method-aware). The response shape depends on `method`:
// passkey -> `{ passkey: CreationChallenge }`; password -> the OPAQUE registration
// response plus a TOTP provisioning URI. Nothing is persisted server-side yet.
export function bootstrapBegin(body: Record<string, unknown>): Promise<Record<string, unknown>> {
  return apiClient
    .post('api/v1/auth/bootstrap/begin', { json: body })
    .json<Record<string, unknown>>()
}

// First-run claim, step 2 (method-aware). On success the API has atomically created
// the org, user, and credential and set the session cookie, and returns the
// one-time recovery codes for the owner to save.
export function bootstrapFinish(body: Record<string, unknown>): Promise<{ recovery_codes: string[] }> {
  return apiClient
    .post('api/v1/auth/bootstrap/finish', { json: body })
    .json<{ recovery_codes: string[] }>()
}

type MintInviteRequest = {
  role?: string
  team_id?: string
  email?: string
}

type MintInviteResponse = {
  token: string
  url: string
  role: string
  expires_at: string
}

// Mints a copyable invite link. The returned `url` is the secret: the API stores
// only its hash, so this is the one time it can be shown.
export function mintInvite(body: MintInviteRequest = {}): Promise<MintInviteResponse> {
  return apiClient
    .post('api/v1/auth/invites', { json: body })
    .json<MintInviteResponse>()
}

type InvitePreview = {
  org_name: string
  team_name: string
  role: string
  email: string
}

// Previews a signed invite. A missing, expired, or already-used token is a
// generic 404, indistinguishable by design.
export function previewInvite(token: string): Promise<InvitePreview> {
  return apiClient
    .get(`api/v1/auth/invites/${token}`)
    .json<InvitePreview>()
}

// Lost-device sign-in: redeems a one-time recovery code for a session. An
// unknown email and a wrong code are the same generic 401.
export function recoveryAuthenticate(email: string, code: string): Promise<Response> {
  return apiClient.post('api/v1/auth/recovery/authenticate', {
    json: { email, code },
  })
}

// Authenticated passkey enrollment (Security Center "add a passkey", and the
// post-recovery walk to a fresh credential). Runs the native registration
// ceremony against the add-credential endpoints.
export async function enrollPasskey(): Promise<void> {
  const challenge = await apiClient
    .post('api/v1/credentials/enroll/begin', { json: { method: 'passkey' }})
    .json<{ passkey: CreationChallenge }>()

  const credential = await startRegistration({ optionsJSON: challenge.passkey.publicKey })

  await apiClient.post('api/v1/credentials/enroll/finish', { json: { credential }})
}
