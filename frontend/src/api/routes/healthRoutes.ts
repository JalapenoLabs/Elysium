import type { HealthResponse, HelloResponse, DatabaseHealthResponse } from '../../proto/generated/health_pb'

// Misc
import { getProto } from '../index'
import {
  HealthResponseSchema,
  HelloResponseSchema,
  DatabaseHealthResponseSchema,
} from '../../proto/generated/health_pb'

export function getHealth(): Promise<HealthResponse> {
  return getProto('health', HealthResponseSchema)
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
