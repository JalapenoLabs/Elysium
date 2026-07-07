// Copyright © 2026 Jalapeno Labs

import type { MeResponse } from '../../proto/generated/auth_pb'

// Misc
import { getProto } from '../index'
import { MeResponseSchema } from '../../proto/generated/auth_pb'

// Fetches the authenticated principal. Protected: the request carries the
// Keycloak bearer token (attached by the Ky hook) and the API returns 401 without
// a valid one.
export function getMe(): Promise<MeResponse> {
  return getProto('api/v1/me', MeResponseSchema)
}
