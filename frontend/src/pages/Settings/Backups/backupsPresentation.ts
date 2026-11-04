// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  BackupsStatus,
  DatabaseBackup,
  DatabaseBackupStatus,
  DatabaseBackupTrigger,
} from '../../../api/routes/backupRoutes'

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const statusLabelKeys = {
  running: 'statuses.running',
  succeeded: 'statuses.succeeded',
  failed: 'statuses.failed',
} as const satisfies Record<DatabaseBackupStatus, ParseKeys<'backups'>>

export const statusChipColors = {
  running: 'accent',
  succeeded: 'success',
  failed: 'danger',
} as const satisfies Record<DatabaseBackupStatus, ChipColor>

export const triggerLabelKeys = {
  schedule: 'triggers.schedule',
  admin: 'triggers.admin',
  deployment: 'triggers.deployment',
} as const satisfies Record<DatabaseBackupTrigger, ParseKeys<'backups'>>

// The newest backup that is stored, and whether one is running now, from the history, which
// is newest first.
export function summarizeHistory(history: DatabaseBackup[]) {
  let lastStored: DatabaseBackup | null = null
  let isRunning = false
  for (const backup of history) {
    if (backup.status === 'running') {
      isRunning = true
      continue
    }
    if (backup.status === 'succeeded') {
      lastStored = backup
      break
    }
  }
  return { lastStored, isRunning } as const
}

// Why Back up now cannot be pressed, or null when it can.
export function backUpNowBlockedKey(status: BackupsStatus): ParseKeys<'backups'> | null {
  if (!status.destination) {
    return 'actions.blocked.noDestination'
  }
  if (summarizeHistory(status.history).isRunning) {
    return 'actions.blocked.running'
  }
  return null
}
