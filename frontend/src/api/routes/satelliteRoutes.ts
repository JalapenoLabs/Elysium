// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `SatelliteStatus` in api/src/fleet/views.rs: what the API's last poll of
// the satellite found.
export type SatelliteStatus = {
  satelliteId: string
  reachable: boolean
  version: string | null
  runningThreads: number | null
  maxConcurrentThreads: number | null
  // Why the satellite is unreachable.
  error: string | null
}

// The API never returns the bearer secret. `status` is null for an inactive
// satellite and until the first poll after a change.
export type Satellite = {
  id: string
  name: string
  description: string
  url: string
  isActive: boolean
  createdAt: string
  updatedAt: string
  status: SatelliteStatus | null
}

type ListSatellitesResponse = {
  satellites: Satellite[]
}

export function listSatellites() {
  return apiClient
    .get('v1/satellites')
    .json<ListSatellitesResponse>()
}

type CreateSatelliteRequest = {
  name: string
  description?: string
  url: string
  secret: string
  isActive?: boolean
}

type CreateSatelliteResponse = {
  satellite: Satellite
}

export function createSatellite(body: CreateSatelliteRequest) {
  return apiClient
    .post('v1/satellites', { json: body })
    .json<CreateSatelliteResponse>()
}

// Omitted fields are left unchanged.
type UpdateSatelliteRequest = {
  name?: string
  description?: string
  url?: string
  secret?: string
  isActive?: boolean
}

type UpdateSatelliteResponse = {
  satellite: Satellite
}

export function updateSatellite(satelliteId: string, body: UpdateSatelliteRequest) {
  return apiClient
    .patch(`v1/satellites/${satelliteId}`, { json: body })
    .json<UpdateSatelliteResponse>()
}

export function deleteSatellite(satelliteId: string) {
  return apiClient
    .delete(`v1/satellites/${satelliteId}`)
}

type TestSatelliteResponse = {
  result: {
    version: string
    protoMajor: number
    protoMinor: number
    runningThreads: number
    maxConcurrentThreads: number
  }
}

export function testSatellite(satelliteId: string) {
  return apiClient
    .post(`v1/satellites/${satelliteId}/test`)
    .json<TestSatelliteResponse>()
}
