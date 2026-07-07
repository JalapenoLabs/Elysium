import type { Options } from 'ky'
import type { Message } from '@bufbuild/protobuf'
import type { GenMessage } from '@bufbuild/protobuf/codegenv2'

// Core
import ky from 'ky'

// Lib
import { fromBinary } from '@bufbuild/protobuf'

// Misc
import { API_BASE_URL } from '../constants'

// Shared Ky instance. Route modules under `./routes` build on top of this.
export const apiClient = ky.create({
  // Ky 2.0 uses the web-standard `baseUrl`; relative inputs like 'api/v1/hello'
  // resolve against it.
  baseUrl: API_BASE_URL,
  // Local dev talks to a single API instance; skip retries so failures surface
  // immediately instead of stalling the UI.
  retry: 0,
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
