// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `DatabaseBackupStatus` in api/src/models/database_backup.rs.
export type DatabaseBackupStatus = 'running' | 'succeeded' | 'failed'

// Mirrors `DatabaseBackupTrigger` in api/src/models/database_backup.rs.
export type DatabaseBackupTrigger = 'schedule' | 'admin' | 'deployment'

// Mirrors `DatabaseBackupView` in api/src/backups/mod.rs: one attempt.
export type DatabaseBackup = {
  id: string
  trigger: DatabaseBackupTrigger
  status: DatabaseBackupStatus
  // Where the files went; null once that location was deleted.
  storageLocationId: string | null
  startedAt: string
  finishedAt: string | null
  // Every dump together, once it succeeded.
  sizeBytes: number | null
  // Why it failed, once it did.
  error: string | null
  createdBy: string
}

// Mirrors `BackupsStatus` in api/src/backups/mod.rs.
export type BackupsStatus = {
  // The location backups go to; null while none is chosen and nothing is backed up.
  destination: { id: string, name: string } | null
  // Elysium's own database first, then Kratos's and Hydra's.
  databases: string[]
  retentionDays: number
  // When the schedule starts its next attempt; null while one runs or no location is chosen.
  nextAttemptAt: string | null
  // Newest first.
  history: DatabaseBackup[]
}

type BackupsResponse = {
  backups: BackupsStatus
}

export function getBackups() {
  return apiClient
    .get('v1/backups')
    .json<BackupsResponse>()
}

type StartBackupResponse = {
  backup: DatabaseBackup
}

// Backs up now. Answers once the backup began; how it ends arrives as `backups.status`.
export function startBackup() {
  return apiClient
    .post('v1/backups')
    .json<StartBackupResponse>()
}

// Chooses the storage location backups go to, or null to stop backing up. The API reaches the
// location first and answers 502 with the provider's message when it cannot.
export function chooseBackupDestination(storageLocationId: string | null) {
  return apiClient
    .put('v1/backups/destination', { json: { storageLocationId }})
    .json<BackupsResponse>()
}
