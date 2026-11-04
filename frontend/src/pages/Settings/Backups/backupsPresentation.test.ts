// Copyright © 2026 Jalapeno Labs

import type { BackupsStatus, DatabaseBackup, DatabaseBackupStatus } from '../../../api/routes/backupRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { backUpNowBlockedKey, summarizeHistory } from './backupsPresentation'

function makeBackup(id: string, status: DatabaseBackupStatus): DatabaseBackup {
  return {
    id,
    trigger: 'schedule',
    status,
    storageLocationId: 'location-1',
    startedAt: '2026-11-03T08:00:00Z',
    finishedAt: status === 'running'
      ? null
      : '2026-11-03T08:02:00Z',
    sizeBytes: status === 'succeeded'
      ? 2048
      : null,
    error: status === 'failed'
      ? 'Bunny refused the password'
      : null,
    createdBy: 'system',
  }
}

function makeStatus(overrides: Partial<BackupsStatus> = {}): BackupsStatus {
  return {
    destination: { id: 'location-1', name: 'Offsite' },
    databases: [ 'elysium', 'kratos', 'hydra' ],
    retentionDays: 30,
    nextAttemptAt: '2026-11-04T08:00:00Z',
    history: [],
    ...overrides,
  }
}

describe('summarizeHistory', () => {
  it('finds the newest stored backup past failures and a running one', () => {
    const history = [
      makeBackup('running', 'running'),
      makeBackup('failed', 'failed'),
      makeBackup('stored', 'succeeded'),
      makeBackup('older', 'succeeded'),
    ]
    const summary = summarizeHistory(history)
    expect(summary.lastStored?.id).toBe('stored')
    expect(summary.isRunning).toBe(true)
  })

  it('reports nothing stored when every backup failed', () => {
    const summary = summarizeHistory([ makeBackup('failed', 'failed') ])
    expect(summary.lastStored).toBeNull()
    expect(summary.isRunning).toBe(false)
  })
})

describe('backUpNowBlockedKey', () => {
  it('blocks a backup with nowhere to go', () => {
    expect(backUpNowBlockedKey(makeStatus({ destination: null }))).toBe('actions.blocked.noDestination')
  })

  it('blocks a second backup while one runs', () => {
    const status = makeStatus({ history: [ makeBackup('running', 'running') ]})
    expect(backUpNowBlockedKey(status)).toBe('actions.blocked.running')
  })

  it('allows a backup to a chosen location when none runs', () => {
    const status = makeStatus({ history: [ makeBackup('stored', 'succeeded') ]})
    expect(backUpNowBlockedKey(status)).toBeNull()
  })
})
