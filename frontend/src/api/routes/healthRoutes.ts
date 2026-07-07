// Copyright © 2026 Jalapeno Labs

import type { HelloResponse, DatabaseHealthResponse } from '../../proto/generated/health_pb'

// Misc
import { apiClient, getProto } from '../index'
import { HelloResponseSchema, DatabaseHealthResponseSchema } from '../../proto/generated/health_pb'

type HealthResponse = {
  status: string
  service: string
}

// Infra health endpoint: plain JSON, not part of the protobuf contract.
export function getHealth(): Promise<HealthResponse> {
  return apiClient.get('health').json<HealthResponse>()
}

export function getHello(): Promise<HelloResponse> {
  return getProto('api/v1/hello', HelloResponseSchema)
}

export function getDatabaseHealth(): Promise<DatabaseHealthResponse> {
  // The API answers 503 with a populated proto body when Postgres is unreachable,
  // so decode the body rather than letting Ky throw on the non-2xx status.
  return getProto('api/v1/db/health', DatabaseHealthResponseSchema, {
    throwHttpErrors: false,
  })
}
