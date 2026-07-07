// Misc
import { apiClient } from '../index'

type HealthResponse = {
  status: string
  service: string
}

export function getHealth() {
  return apiClient
    .get('health')
    .json<HealthResponse>()
}

type HelloResponse = {
  message: string
}

export function getHello() {
  return apiClient
    .get('api/v1/hello')
    .json<HelloResponse>()
}

type DatabaseHealthResponse = {
  database: string
  error?: string
}

export function getDatabaseHealth() {
  return apiClient
    .get('api/v1/db/health')
    .json<DatabaseHealthResponse>()
}
