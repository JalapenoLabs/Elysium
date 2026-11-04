// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { DatabaseBackup } from '../api/routes/backupRoutes'

// Core
import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { Provider } from 'react-redux'
import { MemoryRouter } from 'react-router'

// Redux
import { createAppStore } from '../store'

// User interface
import { BackupHistoryTable } from '../pages/Settings/Backups/BackupHistoryTable'
import { MailAccountTable } from '../pages/Settings/Email/MailAccountTable'
import { LlmTable } from '../pages/Settings/Llms/LlmTable'
import { UserTable } from '../pages/Settings/Users/UserTable'

// Misc
import { makeLlm, makeMailAccount, makeUser } from '../testFixtures'

// Several tables sorted a column by the text it shows rather than the value behind it, so an
// April date came before a March one, priority 10 before priority 9, and a 10 MB backup before a
// 9 KB one. Each of these columns now sorts by its value, whatever its words read.

// Noon UTC, so each day is the same in every zone a test machine might run in. "April" sorts
// before "March" as text, and after it in time.
const MARCH = '2027-03-01T12:00:00Z'
const APRIL = '2027-04-02T12:00:00Z'

const noop = () => {}
const asyncNoop = async () => {}

function renderTable(table: ReactNode) {
  return render(<Provider store={createAppStore()}>
    <MemoryRouter>{table}</MemoryRouter>
  </Provider>)
}

// Presses a column's header until it sorts ascending: number columns start descending.
function sortAscending(columnLabel: string) {
  const button = screen.getByText(columnLabel).closest('button')
  if (!button) {
    throw new Error(`The ${columnLabel} column cannot be sorted`)
  }
  for (let press = 0; press < 3 && button.getAttribute('aria-label') !== 'Sorted ascending'; press++) {
    fireEvent.click(button)
  }
  expect(button.getAttribute('aria-label')).toBe('Sorted ascending')
}

// The rows' names, top to bottom.
function rowOrder(names: string[]) {
  return screen
    .getAllByText(new RegExp(`^(${names.join('|')})$`))
    .map((element) => element.textContent)
}

describe('tables sort by value, not by the text they show', () => {
  it('sorts LLMs by when they expire', () => {
    renderTable(<LlmTable
      llms={[
        makeLlm({ id: 'april', name: 'April key', expiresAt: APRIL }),
        makeLlm({ id: 'march', name: 'March key', expiresAt: MARCH }),
        makeLlm({ id: 'never', name: 'Lasting key', expiresAt: null }),
      ]}
      onToggleActive={asyncNoop}
      onDelete={noop}
    />)

    sortAscending('Expires')
    expect(rowOrder([ 'April key', 'March key', 'Lasting key' ])).toEqual([ 'March key', 'April key', 'Lasting key' ])
  })

  it('sorts LLMs by priority as a number', () => {
    renderTable(<LlmTable
      llms={[
        makeLlm({ id: 'ten', name: 'Ten', priority: 10 }),
        makeLlm({ id: 'nine', name: 'Nine', priority: 9 }),
      ]}
      onToggleActive={asyncNoop}
      onDelete={noop}
    />)

    sortAscending('Priority')
    expect(rowOrder([ 'Ten', 'Nine' ])).toEqual([ 'Nine', 'Ten' ])
  })

  it('sorts mailboxes by when they were last checked, never checked first', () => {
    renderTable(<MailAccountTable
      accounts={[
        makeMailAccount({ id: 'april', address: 'april@example.com', lastCheckedAt: APRIL }),
        makeMailAccount({ id: 'march', address: 'march@example.com', lastCheckedAt: MARCH }),
        makeMailAccount({ id: 'never', address: 'never@example.com', lastCheckedAt: null }),
      ]}
      onTest={asyncNoop}
      onSendTest={asyncNoop}
      onDisconnect={noop}
    />)

    sortAscending('Last checked')
    expect(rowOrder([ 'april@example.com', 'march@example.com', 'never@example.com' ])).toEqual([
      'never@example.com',
      'march@example.com',
      'april@example.com',
    ])
  })

  it('sorts people by when they were last seen, never seen first', () => {
    renderTable(<UserTable
      users={[
        makeUser({ id: 'april', name: 'Ada', lastSeenAt: APRIL }),
        makeUser({ id: 'march', name: 'Grace', lastSeenAt: MARCH }),
        makeUser({ id: 'never', name: 'Linus', lastSeenAt: null }),
      ]}
      roleChanges={{}}
      onChangeRole={noop}
      onSetDisabled={noop}
      onRevokeSessions={noop}
      onResetMfa={noop}
      onCreateRecoveryLink={noop}
    />)

    sortAscending('Last seen')
    expect(rowOrder([ 'Ada', 'Grace', 'Linus' ])).toEqual([ 'Linus', 'Grace', 'Ada' ])
  })

  it('sorts backups by size in bytes', () => {
    const backup = (id: string, sizeBytes: number, error: string): DatabaseBackup => ({
      id,
      trigger: 'schedule',
      status: 'succeeded',
      storageLocationId: null,
      startedAt: MARCH,
      finishedAt: MARCH,
      sizeBytes,
      // Names the row, since nothing else in it differs.
      error,
      createdBy: 'system',
    })
    renderTable(<BackupHistoryTable
      backups={[ backup('large', 10_000_000, 'Large'), backup('small', 9_000, 'Small') ]}
      locations={[]}
    />)

    sortAscending('Size')
    expect(rowOrder([ 'Large', 'Small' ])).toEqual([ 'Small', 'Large' ])
  })
})
