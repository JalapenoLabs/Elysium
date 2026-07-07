// Copyright © 2026 Jalapeno Labs

import type { Options } from 'ky'
import type { Message } from '@bufbuild/protobuf'
import type { GenMessage } from '@bufbuild/protobuf/codegenv2'

// Core
import ky from 'ky'

// Lib
import { fromBinary } from '@bufbuild/protobuf'

// Misc
import { keycloak } from '../auth/keycloak'
import { API_BASE_URL } from '../constants'

// Refresh the token when it has less than this many seconds left before attaching.
const TOKEN_MIN_VALIDITY_SECONDS = 30

// Shared Ky instance. Route modules under `./routes` build on top of this.
export const apiClient = ky.create({
  // Ky 2.0 uses the web-standard `baseUrl`; relative inputs like 'api/v1/hello'
  // resolve against it.
  baseUrl: API_BASE_URL,
  // Local dev talks to a single API instance; skip retries so failures surface
  // immediately instead of stalling the UI.
  retry: 0,
  hooks: {
    beforeRequest: [
      async ({ request }) => {
        // Anonymous requests (health, hello) go out without a token; only attach
        // one once the user has authenticated with Keycloak.
        if (!keycloak.authenticated) {
          return
        }
        try {
          await keycloak.updateToken(TOKEN_MIN_VALIDITY_SECONDS)
          request.headers.set('Authorization', `Bearer ${keycloak.token}`)
        }
        catch (error) {
          console.debug('token refresh failed; sending request without auth', error)
        }
      },
    ],
  },
})

// Fetches a binary protobuf body and decodes it against its generated schema.
//
// This is the client half of the typed contract: the Rust API encodes a message,
// this reads the raw bytes and hydrates the matching generated type. Options pass
// straight through to Ky (e.g. `throwHttpErrors: false` to decode error bodies).
export async function getProto<Shape extends Message>(
  path: string,
  schema: GenMessage<Shape>,
  options?: Options,
): Promise<Shape> {
  const buffer = await apiClient.get(path, options).arrayBuffer()
  return fromBinary(schema, new Uint8Array(buffer))
}
