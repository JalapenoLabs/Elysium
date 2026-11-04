// Copyright © 2026 Jalapeno Labs

import type { User } from './userRoutes'

// Misc
import { apiClient } from '../index'

// What the sign-in and sign-up pages need before anyone is signed in.
export type AuthStatus = {
  // False once an admin closed sign-up. Always true until someone has signed up.
  signupOpen: boolean
  // Nobody has signed up yet: whoever does becomes the admin.
  firstSignUp: boolean
}

export function getAuthStatus() {
  return apiClient
    .get('v1/auth/status')
    .json<AuthStatus>()
}

// The person signed in, and what stands between them and the workspace.
export type Me = {
  user: User
  // Signed in with an authenticator app or lookup code as well as a password or passkey.
  hasSecondFactor: boolean
  // The workspace requires an authenticator app, and this person has none yet.
  mfaEnrollmentRequired: boolean
  // What this person named their authenticator app, such as "Bitwarden". Shown only while
  // Kratos says the app is set up.
  authenticatorNickname: string | null
}

export function getMe() {
  return apiClient
    .get('v1/me')
    .json<Me>()
}

// Ends the session this browser holds and clears its cookie. Asked of the API rather than
// Kratos, so the API forgets the session at once. See docs/auth.md.
export function signOutSession() {
  return apiClient
    .post('v1/auth/sign-out')
}

// Ends one of this person's other sessions, as Kratos lists them.
export function endSession(sessionId: string) {
  return apiClient
    .delete(`v1/me/sessions/${sessionId}`)
}

// Ends every session of this person but this browser's.
export function endOtherSessions() {
  return apiClient
    .delete('v1/me/sessions')
}

// Answers 409 when Kratos holds no authenticator app to name.
export function setAuthenticatorNickname(nickname: string) {
  return apiClient
    .put('v1/me/authenticator-nickname', { json: { nickname }})
}

export function clearAuthenticatorNickname() {
  return apiClient
    .delete('v1/me/authenticator-nickname')
}
