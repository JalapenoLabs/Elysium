// Copyright © 2026 Jalapeno Labs

import type { SessionContext } from '../../proto/generated/session_pb'

// Lib
import { fromBinary } from '@bufbuild/protobuf'

// Misc
import { apiClient } from '../index'
import { SessionContextSchema } from '../../proto/generated/session_pb'

// The one-call session bootstrap: the active account's full profile, every
// account in this browser session, the active account's orgs, and the realtime
// high-water mark (`as_of_sequence`). Also returned base64-encoded so the
// SharedWorker can hold it as the snapshot for future windows.
export async function getSessionContext() {
  const buffer = await apiClient.get('api/v1/context').arrayBuffer()
  const bytes = new Uint8Array(buffer)
  const context: SessionContext = fromBinary(SessionContextSchema, bytes)

  let binaryString = ''
  for (const byte of bytes) {
    binaryString += String.fromCharCode(byte)
  }

  return { context, encoded: btoa(binaryString) } as const
}

// Flips the active account to another one already held by this session. The
// caller re-resolves the session afterward; `/api/v1/me` then answers for the
// newly active account.
export function switchAccount(userId: string): Promise<Response> {
  return apiClient.post('api/v1/session/switch', {
    json: { user_id: userId },
  })
}

// Flips the active org within the active account (400 if it isn't a member).
export function switchOrg(orgId: string): Promise<Response> {
  return apiClient.post('api/v1/session/switch-org', {
    json: { org_id: orgId },
  })
}
