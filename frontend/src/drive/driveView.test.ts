// Copyright © 2026 Jalapeno Labs

import type { DriveView } from './driveView'

// Core
import { describe, expect, it } from 'vitest'
import { create } from '@bufbuild/protobuf'

// Misc
import { driveHasCapability, splitDrivesByCapability, toDriveView } from './driveView'
import { DriveKind, DriveSchema, StorageCapability } from '../proto/generated/storage_pb'

function makeDrive(name: string, kind: DriveKind, capabilities: StorageCapability[]): DriveView {
  return {
    id: name,
    projectId: 'p1',
    name,
    kind,
    isDefault: false,
    credentialSet: false,
    capabilities,
  }
}

const localDrive = makeDrive('Local', DriveKind.FS, [
  StorageCapability.PLAIN_FILES,
  StorageCapability.OBJECT_STORE,
  StorageCapability.POSIX_FS,
])
const bucketDrive = makeDrive('Bucket', DriveKind.S3, [
  StorageCapability.PLAIN_FILES,
  StorageCapability.OBJECT_STORE,
])
const sftpDrive = makeDrive('Vault', DriveKind.SFTP, [ StorageCapability.PLAIN_FILES ])

describe('toDriveView', () => {
  it('projects the generated message, never carrying a credential', () => {
    const message = create(DriveSchema, {
      id: { value: 'd1' },
      projectId: { value: 'p1' },
      name: 'Team Drive',
      kind: DriveKind.S3,
      isDefault: true,
      credentialSet: true,
      capabilities: [ StorageCapability.PLAIN_FILES, StorageCapability.OBJECT_STORE ],
    })

    expect(toDriveView(message)).toEqual({
      id: 'd1',
      projectId: 'p1',
      name: 'Team Drive',
      kind: DriveKind.S3,
      isDefault: true,
      credentialSet: true,
      capabilities: [ StorageCapability.PLAIN_FILES, StorageCapability.OBJECT_STORE ],
    })
  })
})

describe('driveHasCapability', () => {
  it('enforces the capability matrix from docs/storage.md', () => {
    // POSIX_FS is local-FS-only: the "git on OneDrive is impossible" rule.
    expect(driveHasCapability(localDrive, StorageCapability.POSIX_FS)).toBe(true)
    expect(driveHasCapability(bucketDrive, StorageCapability.POSIX_FS)).toBe(false)
    expect(driveHasCapability(sftpDrive, StorageCapability.OBJECT_STORE)).toBe(false)
    expect(driveHasCapability(sftpDrive, StorageCapability.PLAIN_FILES)).toBe(true)
  })
})

describe('splitDrivesByCapability', () => {
  it('partitions while preserving roster order', () => {
    const { eligible, ineligible } = splitDrivesByCapability(
      [ sftpDrive, localDrive, bucketDrive ],
      StorageCapability.OBJECT_STORE,
    )
    expect(eligible.map((drive) => drive.name)).toEqual([ 'Local', 'Bucket' ])
    expect(ineligible.map((drive) => drive.name)).toEqual([ 'Vault' ])
  })

  it('puts every drive in eligible for PLAIN_FILES (every provider has it)', () => {
    const { eligible, ineligible } = splitDrivesByCapability(
      [ sftpDrive, localDrive, bucketDrive ],
      StorageCapability.PLAIN_FILES,
    )
    expect(eligible).toHaveLength(3)
    expect(ineligible).toHaveLength(0)
  })
})
