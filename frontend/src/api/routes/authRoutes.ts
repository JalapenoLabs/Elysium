// Copyright © 2026 Jalapeno Labs

import type { MeResponse } from '../../proto/generated/auth_pb'

// Redux
import { dispatch } from '../../store'
import { sessionAnonymous } from '../../store/sessionSlice'

// Misc
import { apiClient, getProto } from '../index'
import { MeResponseSchema } from '../../proto/generated/auth_pb'

// Fetches the authenticated principal. Protected: the request carries the session
// cookie (attached automatically) and the API returns 401 without a valid one.
// This doubles as the session probe on load: a 200 means authenticated, a 401
// means anonymous.
export function getMe(): Promise<MeResponse> {
  return getProto('api/v1/me', MeResponseSchema)
}

// Ends the session. The API clears the session cookie and ends the Keycloak
// session server-side. The browser holds no token, so there is nothing to discard
// client-side beyond the local Redux state, which this clears in a `finally` so it
// happens wherever `logout` is called, even if the request itself fails.
export async function logout(): Promise<Response> {
  try {
    return await apiClient.post('api/v1/auth/logout')
  }
  finally {
    dispatch(sessionAnonymous())
  }
}
