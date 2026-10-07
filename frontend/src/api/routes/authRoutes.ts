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
}

export function getMe() {
  return apiClient
    .get('v1/me')
    .json<Me>()
}
