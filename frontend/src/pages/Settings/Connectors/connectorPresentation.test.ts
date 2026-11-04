// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Utility
import i18next from 'i18next'

// Misc
import {
  connectorOwningBrowser,
  describeOutcome,
  filterConnectors,
  hasRequiredFailure,
  pickedIds,
  searchKinds,
  startingContent,
  summarize,
} from './connectorPresentation'
import { makeConnector } from '../../../testFixtures'

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

describe('searchKinds', () => {
  const words = {
    jira: 'Jira Issues on a Jira Cloud site',
    outlook: 'Outlook Your Outlook inbox, read as mail',
    gmail: 'Gmail Your Gmail inbox, read as mail',
  } as const
  const kinds = [ 'jira', 'outlook', 'gmail' ] as const

  it('keeps every kind, in order, for an empty search', () => {
    expect(searchKinds([ ...kinds ], '  ', (kind) => words[kind as keyof typeof words])).toEqual(kinds)
  })

  it('matches a kind by its name or what it reads, ignoring case', () => {
    expect(searchKinds([ ...kinds ], 'MAIL', (kind) => words[kind as keyof typeof words]))
      .toEqual([ 'outlook', 'gmail' ])
    expect(searchKinds([ ...kinds ], 'jira', (kind) => words[kind as keyof typeof words])).toEqual([ 'jira' ])
  })
})

describe('startingContent', () => {
  it('reads everything each selection can pick, and leaves other fields to be chosen', () => {
    expect(startingContent({
      content: [
        { id: 'conversations', field: { type: 'selection' }, required: true },
        { id: 'storageLocationId', field: { type: 'storage_location' }, required: true },
      ],
    })).toEqual({ conversations: '*' })
  })

  it('reads nothing chosen for a kind that reads its whole account', () => {
    expect(startingContent({ content: []})).toEqual({})
  })
})

describe('connectorOwningBrowser', () => {
  const outlook = makeConnector({
    id: 'outlook',
    kind: 'outlook',
    account: { id: 'browser-1', kind: 'browser', name: 'outlook' },
  })
  // An account of another kind with the same id is not a browser: the account's kind decides.
  const jira = makeConnector({ id: 'jira', account: { id: 'browser-2', kind: 'atlassian', name: 'Work' }})
  const waiting = makeConnector({ id: 'teams', kind: 'teams', account: null })

  it('finds the connector whose account is the browser', () => {
    expect(connectorOwningBrowser([ jira, waiting, outlook ], 'browser-1')).toBe(outlook)
  })

  it('finds nothing for an id no browser connector owns', () => {
    expect(connectorOwningBrowser([ jira, waiting, outlook ], 'browser-2')).toBeNull()
    expect(connectorOwningBrowser([ jira, waiting, outlook ], 'gone')).toBeNull()
  })
})

describe('pickedIds', () => {
  // A Jira connector's allowlist and a Confluence connector's spaces come back as the provider
  // described each pick; read as text they were "[object Object]", so nothing showed as picked
  // and the next change dropped every earlier pick.
  it('reads a pick stored as the provider described it as its id', () => {
    expect(pickedIds([{ id: '10001', key: 'ELY', name: 'Elysium' }, { id: 7, name: 'Board' }]))
      .toEqual([ '10001', '7' ])
  })

  it('reads bare ids as they are, and anything else as no pick', () => {
    expect(pickedIds([ 'C1', 42 ])).toEqual([ 'C1', '42' ])
    expect(pickedIds('*')).toEqual([])
    expect(pickedIds(undefined)).toEqual([])
  })
})
