// Copyright © 2026 Jalapeno Labs

import type {
  Drive,
  DriveAccess,
  DriveConfig,
  DriveKind,
  DriveVisibility,
  ListDrivesResponse,
} from '../../proto/generated/storage_pb'

// Lib
import { create, fromBinary, toBinary } from '@bufbuild/protobuf'

// Misc
import { apiClient, getProto, sendProto } from '../index'
import {
  CreateDriveRequestSchema,
  DriveSchema,
  ListDrivesResponseSchema,
  SetDriveAccessRequestSchema,
  UpdateDriveRequestSchema,
} from '../../proto/generated/storage_pb'

// Drives: an org's storage locations, project-scoped (docs/storage.md).
// Listing needs project access; every mutation is gated on `manage_storage`.
// The provider credential is write-only: it rides create/update requests and
// is never returned (`Drive.credential_set` reports only that one exists).

// 403 (not an empty list) when the caller cannot reach the project.
export function listDrives(projectId: string): Promise<ListDrivesResponse> {
  return getProto(`api/v1/drives?project_id=${projectId}`, ListDrivesResponseSchema)
}

type CreateDriveFields = {
  projectId: string
  name: string
  kind: DriveKind
  visibility: DriveVisibility
  config: DriveConfig
  // Empty = no provider secret (valid for a local FS Drive).
  credential: string
}

export function createDrive(fields: CreateDriveFields): Promise<Drive> {
  return sendProto('api/v1/drives', {
    method: 'post',
    requestSchema: CreateDriveRequestSchema,
    request: create(CreateDriveRequestSchema, {
      projectId: { value: fields.projectId },
      name: fields.name,
      kind: fields.kind,
      visibility: fields.visibility,
      config: fields.config,
      credential: fields.credential,
    }),
    responseSchema: DriveSchema,
  })
}

type UpdateDriveFields = {
  name: string
  visibility: DriveVisibility
  config: DriveConfig
  // Empty keeps the existing sealed credential.
  credential: string
}

export function updateDrive(driveId: string, fields: UpdateDriveFields): Promise<Drive> {
  return sendProto(`api/v1/drives/${driveId}`, {
    method: 'patch',
    requestSchema: UpdateDriveRequestSchema,
    request: create(UpdateDriveRequestSchema, fields),
    responseSchema: DriveSchema,
  })
}

export function deleteDrive(driveId: string) {
  return apiClient.delete(`api/v1/drives/${driveId}`)
}

// 200 = the provider is reachable; 422 carries a plain-text failure message.
// Returns a result instead of throwing so callers can read the message without
// fighting over the (single-read) response body.
export async function testDrive(driveId: string) {
  const response = await apiClient.post(`api/v1/drives/${driveId}/test`, { throwHttpErrors: false })
  return {
    reachable: response.ok,
    message: response.ok ? '' : await response.text(),
  } as const
}

// Full replace of the Drive's restricted-visibility grants.
export function setDriveAccess(driveId: string, grants: DriveAccess[]) {
  return apiClient.put(`api/v1/drives/${driveId}/access`, {
    body: toBinary(
      SetDriveAccessRequestSchema,
      create(SetDriveAccessRequestSchema, { grants }),
    ),
    headers: { 'content-type': 'application/x-protobuf' },
  })
}

// Body-less POST returning the updated Drive.
export async function setDefaultDrive(driveId: string): Promise<Drive> {
  const buffer = await apiClient.post(`api/v1/drives/${driveId}/default`).arrayBuffer()
  return fromBinary(DriveSchema, new Uint8Array(buffer))
}
