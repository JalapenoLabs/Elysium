// Copyright © 2026 Jalapeno Labs

import type { Drive } from '../proto/generated/storage_pb'

// Misc
import { DriveKind, StorageCapability } from '../proto/generated/storage_pb'

// Serializable projection of `elysium.storage.v1.Drive` for stores and props
// (docs/storage.md). The provider credential never reaches the client; only
// the `credentialSet` boolean rides along.
export type DriveView = {
  id: string
  projectId: string
  name: string
  kind: DriveKind
  isDefault: boolean
  credentialSet: boolean
  capabilities: StorageCapability[]
}

export function toDriveView(drive: Drive): DriveView {
  return {
    id: drive.id?.value ?? '',
    projectId: drive.projectId?.value ?? '',
    name: drive.name,
    kind: drive.kind,
    isDefault: drive.isDefault,
    credentialSet: drive.credentialSet,
    capabilities: drive.capabilities,
  }
}

// Presentational metadata per provider kind; labels live in common.jsonc
// under `drive.kinds.*`.
export const driveKindMeta = {
  [DriveKind.UNSPECIFIED]: { labelKey: 'drive.kinds.unspecified' },
  [DriveKind.FS]: { labelKey: 'drive.kinds.fs' },
  [DriveKind.S3]: { labelKey: 'drive.kinds.s3' },
  [DriveKind.SFTP]: { labelKey: 'drive.kinds.sftp' },
} as const satisfies Record<DriveKind, { labelKey: string }>

export const capabilityLabelKeyByCapability = {
  [StorageCapability.UNSPECIFIED]: 'drive.capabilities.unspecified',
  [StorageCapability.PLAIN_FILES]: 'drive.capabilities.plainFiles',
  [StorageCapability.OBJECT_STORE]: 'drive.capabilities.objectStore',
  [StorageCapability.POSIX_FS]: 'drive.capabilities.posixFs',
} as const satisfies Record<StorageCapability, string>

// The capability rule (docs/storage.md): a consumer may bind a storage role
// only to a Drive whose provider advertises the required capability. This is
// why "git on OneDrive" is an enforced impossibility, and the picker uses it
// to disable ineligible Drives rather than hiding the rule.
export function driveHasCapability(drive: DriveView, capability: StorageCapability): boolean {
  return drive.capabilities.includes(capability)
}

// Splits a roster into eligible and ineligible Drives for a required
// capability, preserving order; the picker renders both so users see why a
// Drive cannot be chosen.
export function splitDrivesByCapability(drives: DriveView[], capability: StorageCapability) {
  const eligible: DriveView[] = []
  const ineligible: DriveView[] = []
  for (const drive of drives) {
    if (driveHasCapability(drive, capability)) {
      eligible.push(drive)
    }
    else {
      ineligible.push(drive)
    }
  }
  return {
    eligible,
    ineligible,
  } as const
}
