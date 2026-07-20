// Copyright © 2026 Jalapeno Labs

import type { Listing } from '../../proto/generated/storage_pb'

// Lib
import { create, toBinary } from '@bufbuild/protobuf'

// Misc
import { apiClient, getProto } from '../index'
import { API_BASE_URL } from '../../constants'
import { ListingSchema, MoveRequestSchema } from '../../proto/generated/storage_pb'

// Files within a Drive (docs/storage.md): everything proxies through the
// backend to the provider; nothing is mirrored. Paths are relative to the
// Drive root; empty = root. `..` is rejected (400) and the `.elysium` system
// area is invisible (404) server-side.

export function listFiles(driveId: string, path: string): Promise<Listing> {
  return getProto(
    `api/v1/drives/${driveId}/files?path=${encodeURIComponent(path)}`,
    ListingSchema,
  )
}

export function deleteEntry(driveId: string, path: string) {
  return apiClient.delete(`api/v1/drives/${driveId}/files?path=${encodeURIComponent(path)}`)
}

// Raw octet-stream body; the backend streams it to the provider. 413 = over
// the org upload cap. No Ky timeout: large uploads take as long as they take.
export function uploadFile(driveId: string, path: string, file: File) {
  return apiClient.put(`api/v1/drives/${driveId}/content?path=${encodeURIComponent(path)}`, {
    body: file,
    timeout: false,
  })
}

// Downloads are browser-streamed: navigating an anchor at this URL lets the
// browser handle progress and disk, with the session cookie riding along.
export function getDownloadUrl(driveId: string, path: string): string {
  return `${API_BASE_URL}/api/v1/drives/${driveId}/content?path=${encodeURIComponent(path)}`
}

export function createFolder(driveId: string, path: string) {
  return apiClient.post(`api/v1/drives/${driveId}/folders?path=${encodeURIComponent(path)}`)
}

export function moveEntry(driveId: string, from: string, to: string) {
  return apiClient.post(`api/v1/drives/${driveId}/move`, {
    body: toBinary(MoveRequestSchema, create(MoveRequestSchema, { from, to })),
    headers: { 'content-type': 'application/x-protobuf' },
  })
}
