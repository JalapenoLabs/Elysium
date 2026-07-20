// Copyright © 2026 Jalapeno Labs

import type { StatusKind } from '../theme/statusColors'

// Misc
import { IntegrationKind, ReconcileStatus } from '../proto/generated/integration_pb'
import { StorageCapability } from '../proto/generated/storage_pb'

// Presentational + binding metadata for integrations (docs/integrations.md).
// Labels live in common.jsonc under `integration.*`.

export const integrationKindMeta = {
  [IntegrationKind.UNSPECIFIED]: { labelKey: 'integration.kinds.unspecified' },
  [IntegrationKind.GIT]: { labelKey: 'integration.kinds.git' },
  [IntegrationKind.MAIL]: { labelKey: 'integration.kinds.mail' },
  [IntegrationKind.REGISTRY]: { labelKey: 'integration.kinds.registry' },
  [IntegrationKind.PERFORCE]: { labelKey: 'integration.kinds.perforce' },
  [IntegrationKind.RUNNER]: { labelKey: 'integration.kinds.runner' },
} as const satisfies Record<IntegrationKind, { labelKey: string }>

// The storage roles each managed kind binds, with the capability the bound
// Drive must advertise (docs/storage.md's capability rule; the server rejects
// ineligible bindings with a 400, the DrivePicker prevents them visually).
export type StorageRole = {
  role: string
  labelKey: string
  capability: StorageCapability
}

export const storageRolesByKind = {
  [IntegrationKind.UNSPECIFIED]: [],
  [IntegrationKind.GIT]: [
    { role: 'repos', labelKey: 'integration.roles.repos', capability: StorageCapability.POSIX_FS },
  ],
  [IntegrationKind.MAIL]: [
    { role: 'bodies', labelKey: 'integration.roles.bodies', capability: StorageCapability.OBJECT_STORE },
  ],
  [IntegrationKind.REGISTRY]: [
    { role: 'blobs', labelKey: 'integration.roles.blobs', capability: StorageCapability.OBJECT_STORE },
  ],
  [IntegrationKind.PERFORCE]: [
    { role: 'metadata', labelKey: 'integration.roles.metadata', capability: StorageCapability.POSIX_FS },
    { role: 'archive', labelKey: 'integration.roles.archive', capability: StorageCapability.OBJECT_STORE },
  ],
  [IntegrationKind.RUNNER]: [
    { role: 'workspace', labelKey: 'integration.roles.workspace', capability: StorageCapability.POSIX_FS },
  ],
} as const satisfies Record<IntegrationKind, readonly StorageRole[]>

// Reconciler state in the product-wide status grammar (docs/ux.md).
export const statusKindByReconcileStatus = {
  [ReconcileStatus.UNSPECIFIED]: 'idle',
  [ReconcileStatus.PENDING]: 'needsInput',
  [ReconcileStatus.PROVISIONING]: 'active',
  [ReconcileStatus.RUNNING]: 'done',
  [ReconcileStatus.STOPPED]: 'idle',
  [ReconcileStatus.FAILED]: 'error',
} as const satisfies Record<ReconcileStatus, StatusKind>

export const reconcileStatusLabelKey = {
  [ReconcileStatus.UNSPECIFIED]: 'integration.status.unspecified',
  [ReconcileStatus.PENDING]: 'integration.status.pending',
  [ReconcileStatus.PROVISIONING]: 'integration.status.provisioning',
  [ReconcileStatus.RUNNING]: 'integration.status.running',
  [ReconcileStatus.STOPPED]: 'integration.status.stopped',
  [ReconcileStatus.FAILED]: 'integration.status.failed',
} as const satisfies Record<ReconcileStatus, string>
