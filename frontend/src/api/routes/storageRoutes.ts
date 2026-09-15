// Copyright © 2026 Jalapeno Labs

import type { ProjectScope } from './projectRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `StorageLocationKind` in api/src/models/storage_location.rs.
export const STORAGE_PROVIDER_KINDS = [ 'bunny', 's3' ] as const
export type StorageProviderKind = typeof STORAGE_PROVIDER_KINDS[number]

// Mirrors `S3Service` in api/src/models/storage_location.rs: the services Elysium reaches
// over the S3 API, each at an endpoint fixed in the API.
export const S3_SERVICES = [ 'aws', 'google-cloud' ] as const
export type S3Service = typeof S3_SERVICES[number]

// Mirrors `BunnyStorageRegion` in api/src/models/storage_location.rs.
export const BUNNY_STORAGE_REGIONS = [
  'frankfurt',
  'london',
  'new-york',
  'los-angeles',
  'singapore',
  'stockholm',
  'sao-paulo',
  'johannesburg',
  'sydney',
] as const
export type BunnyStorageRegion = typeof BUNNY_STORAGE_REGIONS[number]

export type BunnyStorageProvider = {
  kind: 'bunny'
  // The storage zone's name.
  zone: string
  region: BunnyStorageRegion
}

export type S3StorageProvider = {
  kind: 's3'
  service: S3Service
  bucket: string
  // The bucket's region, such as us-east-1, for aws; null for google-cloud.
  region: string | null
  // Names the key; the secret half is the location's access key.
  accessKeyId: string
}

// Mirrors `StorageProvider` in api/src/models/storage_location.rs: where a location's
// files go, with the settings only that provider has.
export type StorageProvider = BunnyStorageProvider | S3StorageProvider

// The API never returns the access key.
export type StorageLocation = {
  id: string
  name: string
  provider: StorageProvider
  // The directory Elysium writes under, without surrounding slashes; empty for the root.
  pathPrefix: string
  // Elysium's own cap on what it stores here; null for no limit.
  storageLimitBytes: number | null
  // The projects that save files here.
  projects: ProjectScope
  createdAt: string
  updatedAt: string
}

type ListStorageLocationsResponse = {
  locations: StorageLocation[]
}

export function listStorageLocations() {
  return apiClient
    .get('v1/storage-locations')
    .json<ListStorageLocationsResponse>()
}

type CreateStorageLocationRequest = {
  name: string
  provider: StorageProvider
  pathPrefix?: string
  // Null for no limit.
  storageLimitBytes: number | null
  accessKey: string
  // Absent links no projects yet.
  projects?: ProjectScope
}

type StorageLocationResponse = {
  location: StorageLocation
}

export function createStorageLocation(body: CreateStorageLocationRequest) {
  return apiClient
    .post('v1/storage-locations', { json: body })
    .json<StorageLocationResponse>()
}

// Omitted fields are left unchanged. A provider replaces all of its settings together.
type UpdateStorageLocationRequest = {
  name?: string
  provider?: StorageProvider
  pathPrefix?: string
  // Null removes the limit.
  storageLimitBytes?: number | null
  accessKey?: string
  // Replaces the projects that save files here.
  projects?: ProjectScope
}

export function updateStorageLocation(locationId: string, body: UpdateStorageLocationRequest) {
  return apiClient
    .patch(`v1/storage-locations/${locationId}`, { json: body })
    .json<StorageLocationResponse>()
}

export function deleteStorageLocation(locationId: string) {
  return apiClient
    .delete(`v1/storage-locations/${locationId}`)
}

type TestStorageLocationResponse = {
  result: {
    // Files and directories directly inside the location's directory, up to one page.
    entries: number
    // The directory holds more than `entries`.
    hasMore: boolean
  }
}

export function testStorageLocation(locationId: string) {
  return apiClient
    .post(`v1/storage-locations/${locationId}/test`)
    .json<TestStorageLocationResponse>()
}
