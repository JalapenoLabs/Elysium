// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { buildBackupCodesFile } from './backupCodesFile'

// Local time, as the person downloading sees it: November 4th 2026 at 3:07pm.
const createdAt = new Date(2026, 10, 4, 15, 7)

describe('buildBackupCodesFile', () => {
  it('names the file for the app and the moment it was made', () => {
    const file = buildBackupCodesFile({
      appName: 'Bitwarden (work)',
      account: 'ada@example.com',
      codes: [ 'abcd1234' ],
      createdAt,
    })
    expect(file.filename).toBe('elysium-backup-codes-bitwarden-work-2026-11-04-1507.txt')
  })

  it('says which app and account the codes back up, and when, above the codes', () => {
    const file = buildBackupCodesFile({
      appName: 'Bitwarden',
      account: 'ada@example.com',
      codes: [ 'abcd1234', 'efgh5678' ],
      createdAt,
    })
    const lines = file.contents.split('\n')
    expect(lines).toContain('App: Bitwarden')
    expect(lines).toContain('Account: ada@example.com')
    expect(lines).toContain('Created: November 4th 2026 at 3:07pm')
    expect(lines.slice(-3)).toEqual([ 'abcd1234', 'efgh5678', '' ])
  })

  it('keeps a name made only of symbols readable in the filename', () => {
    const file = buildBackupCodesFile({ appName: '🔐', account: '', codes: [], createdAt })
    expect(file.filename).toBe('elysium-backup-codes-app-2026-11-04-1507.txt')
  })

  it('names an unnamed app as an authenticator app', () => {
    const file = buildBackupCodesFile({ appName: '  ', account: '', codes: [], createdAt })
    expect(file.contents).toContain('App: Authenticator app')
    expect(file.filename).toBe('elysium-backup-codes-authenticator-app-2026-11-04-1507.txt')
  })
})
