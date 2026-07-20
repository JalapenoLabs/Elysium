// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  statusKindByReconcileStatus,
  storageRolesByKind,
} from './model'
import { IntegrationKind, ReconcileStatus } from '../proto/generated/integration_pb'
import { StorageCapability } from '../proto/generated/storage_pb'

describe('storageRolesByKind', () => {
  it('matches the #28 role-to-capability contract exactly', () => {
    const flat = Object.fromEntries(
      Object.values(IntegrationKind)
        .filter((kind): kind is IntegrationKind => typeof kind === 'number')
        .flatMap((kind) => storageRolesByKind[kind].map((role) => [
          `${IntegrationKind[kind]}.${role.role}`,
          role.capability,
        ])),
    )

    expect(flat).toEqual({
      'GIT.repos': StorageCapability.POSIX_FS,
      'MAIL.bodies': StorageCapability.OBJECT_STORE,
      'REGISTRY.blobs': StorageCapability.OBJECT_STORE,
      'PERFORCE.metadata': StorageCapability.POSIX_FS,
      'PERFORCE.archive': StorageCapability.OBJECT_STORE,
      'RUNNER.workspace': StorageCapability.POSIX_FS,
    })
  })
})

describe('statusKindByReconcileStatus', () => {
  it('maps reconciler states onto the status grammar per docs/ux.md', () => {
    expect(statusKindByReconcileStatus[ReconcileStatus.PROVISIONING]).toBe('active')
    expect(statusKindByReconcileStatus[ReconcileStatus.RUNNING]).toBe('done')
    expect(statusKindByReconcileStatus[ReconcileStatus.FAILED]).toBe('error')
    // PENDING means the admin owes it something (an unbound role): their turn.
    expect(statusKindByReconcileStatus[ReconcileStatus.PENDING]).toBe('needsInput')
  })
})
