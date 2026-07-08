// Copyright © 2026 Jalapeno Labs

import type { MeResponse } from '../../proto/generated/auth_pb'
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
type CreationChallenge = { publicKey: PublicKeyCredentialCreationOptionsJSON }
type RequestChallenge = { publicKey: PublicKeyCredentialRequestOptionsJSON }

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

// First-run: claim an unclaimed instance and enroll the root user's first passkey.
//
// Posts the identity, runs the native registration ceremony in the browser
// (`navigator.credentials.create`, surfaced by the OS, not a webpage), and posts
// the resulting credential back. On success the API has stored the passkey and set
// the session cookie; the caller re-resolves the session.
export async function bootstrapPasskey(
  email: string,
  displayName: string,
  orgName: string,
): Promise<void> {
  const challenge = await apiClient
    .post('api/v1/auth/bootstrap/begin', {
      json: { email, display_name: displayName, org_name: orgName },
    })
    .json<CreationChallenge>()

  const credential = await startRegistration({ optionsJSON: challenge.publicKey })

  await apiClient.post('api/v1/auth/bootstrap/finish', { json: credential })
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
