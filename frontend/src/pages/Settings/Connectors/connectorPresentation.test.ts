// Copyright © 2026 Jalapeno Labs

import type { Connector } from '../../../api/routes/connectorRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Utility
import i18next from 'i18next'

// Misc
import {
  describeOutcome,
  filterConnectors,
  hasRequiredFailure,
  offeredAccounts,
  summarize,
} from './connectorPresentation'

function makeConnector(overrides: Partial<Connector> & Pick<Connector, 'id'>): Connector {
  return {
    kind: 'jira',
    scope: 'workspace',
    account: { id: 'account-1', kind: 'atlassian', name: 'Work' },
    status: 'active',
    isActive: true,
    signInState: 'unknown',
    lastSyncAt: null,
    lastError: null,
    records: 0,
    content: {},
    storageLocationId: null,
    checks: [],
    lastRun: null,
    createdBy: 'person-1',
    createdAt: '2026-11-02T12:00:00Z',
    ...overrides,
  }
}

const t = i18next.getFixedT('en-US', 'connectors')

describe('summarize', () => {
  it('counts every connector but the paused as active, and adds up their records', () => {
    const summary = summarize([
      makeConnector({ id: 'one', records: 3 }),
      makeConnector({ id: 'two', status: 'error', records: 2 }),
      makeConnector({ id: 'three', status: 'paused', isActive: false }),
    ])
    expect(summary).toEqual({ total: 3, active: 2, errors: 1, records: 5 })
  })
})

describe('filterConnectors', () => {
  const connectors = [
    makeConnector({ id: 'jira' }),
    makeConnector({ id: 'mail', kind: 'gmail', account: { id: 'browser-1', kind: 'browser', name: 'Personal' }}),
    makeConnector({ id: 'broken', kind: 'github', status: 'error', account: null }),
  ]
  const kindName = (kind: string) => kind.toUpperCase()

  it('matches the kind and the account name, in any case', () => {
    expect(filterConnectors(connectors, 'gmail', 'all', kindName).map((kept) => kept.id)).toEqual([ 'mail' ])
    expect(filterConnectors(connectors, 'WORK', 'all', kindName).map((kept) => kept.id)).toEqual([ 'jira' ])
  })

  it('keeps only the status asked for', () => {
    expect(filterConnectors(connectors, '', 'error', kindName).map((kept) => kept.id)).toEqual([ 'broken' ])
  })
})

describe('describeOutcome', () => {
  // The labels the hub shows; only `sign_in` is waited on below.
  const labels: Record<string, string> = { sign_in: 'Sign in' }
  const labelOf = (checkId: string) => labels[checkId] ?? checkId

  it('names the check a skipped one waited for', () => {
    const outcome = { state: 'skipped', count: null, reason: null, message: null, blockedBy: 'sign_in' } as const
    expect(describeOutcome(t, outcome, labelOf, 'list_projects')).toBe('Waits for Sign in')
  })

  it('words a count with its own plural', () => {
    const outcome = { state: 'passed', count: 6, reason: null, message: null, blockedBy: null } as const
    expect(describeOutcome(t, outcome, labelOf, 'list_projects')).toBe('Reaches 6 projects')
  })

  it('translates Elysium\'s own reason and repeats the provider\'s failure', () => {
    const reasoned = { state: 'skipped', count: null, reason: 'no_messages', message: null, blockedBy: null } as const
    expect(describeOutcome(t, reasoned, labelOf, 'sample_message')).toBe('The inbox shows no messages to sample.')

    const refused = { state: 'failed', count: null, reason: null, message: 'Bad credentials', blockedBy: null } as const
    expect(describeOutcome(t, refused, labelOf, 'sign_in')).toBe('Bad credentials')
  })
})

describe('hasRequiredFailure', () => {
  const specs = [
    { id: 'sign_in', required: true, dependsOn: []},
    { id: 'sample_message', required: false, dependsOn: [ 'sign_in' ]},
  ]
  const base = { count: null, reason: null, message: null, blockedBy: null }

  it('ignores an optional check that did not pass', () => {
    expect(hasRequiredFailure(specs, [
      { ...base, checkId: 'sign_in', state: 'passed' },
      { ...base, checkId: 'sample_message', state: 'skipped' },
    ])).toBe(false)
  })

  it('flags a required check that did not pass', () => {
    expect(hasRequiredFailure(specs, [{ ...base, checkId: 'sign_in', state: 'failed' }])).toBe(true)
  })
})

describe('offeredAccounts', () => {
  const browsers = [{ id: 'browser-1', name: 'Work Outlook' }]
  const accounts = [
    { id: 'atlassian-1', kind: 'atlassian' as const, name: 'Work' },
    { id: 'fireflies-1', kind: 'fireflies' as const, name: 'Fireflies' },
  ]

  it('offers nothing for a kind that always makes its own account', () => {
    expect(offeredAccounts({ account: 'atlassian', accounts: 'new' }, browsers, accounts)).toEqual([])
  })

  it('offers the person\'s browsers for a kind read through one', () => {
    expect(offeredAccounts({ account: 'browser', accounts: 'existing' }, browsers, accounts)).toEqual(browsers)
  })

  // Confluence reads through the Atlassian account a Jira connector made, so the wizard must
  // offer it; before kinds declared their account choice, only Fireflies keys were offered.
  it('offers the hub\'s accounts of the kind\'s account kind', () => {
    expect(offeredAccounts({ account: 'atlassian', accounts: 'existing' }, browsers, accounts))
      .toEqual([{ id: 'atlassian-1', name: 'Work' }])
    expect(offeredAccounts({ account: 'fireflies', accounts: 'either' }, browsers, accounts))
      .toEqual([{ id: 'fireflies-1', name: 'Fireflies' }])
  })
})
