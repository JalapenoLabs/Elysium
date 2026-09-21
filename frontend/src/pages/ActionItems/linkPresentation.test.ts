// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makePendingWrite } from '../../testFixtures'
import {
  buildIssueSearchJql,
  credentialOptions,
  describePendingWrite,
  EMPTY_LINK_TARGET_DRAFT,
  fromCredentialKey,
  toCredentialKey,
  toLinkTarget,
} from './linkPresentation'

describe('buildIssueSearchJql', () => {
  it('finds a typed key by that key alone, even among epics', () => {
    expect(buildIssueSearchJql(' ely-12 ')).toBe('key = "ELY-12"')
    expect(buildIssueSearchJql('ELY-12', true)).toBe('key = "ELY-12"')
  })

  it('matches words, escaping what could close the string', () => {
    expect(buildIssueSearchJql('say "hi" \\ bye')).toBe('text ~ "say \\"hi\\" \\\\ bye" ORDER BY updated DESC')
  })

  it('lists recent issues for an empty search, since Jira refuses an unbounded one', () => {
    expect(buildIssueSearchJql('  ')).toBe('updated >= -90d ORDER BY updated DESC')
  })

  it('narrows to epics', () => {
    expect(buildIssueSearchJql('', true)).toBe('issuetype = Epic ORDER BY updated DESC')
    expect(buildIssueSearchJql('storage', true)).toBe('text ~ "storage" AND issuetype = Epic ORDER BY updated DESC')
  })
})

describe('credential keys', () => {
  it('round-trips a provider and credential', () => {
    const key = toCredentialKey('github', 'abc')
    expect(fromCredentialKey(key)).toEqual({ provider: 'github', credentialId: 'abc' })
  })

  it('refuses a key naming no known provider or no credential', () => {
    expect(fromCredentialKey('')).toBeNull()
    expect(fromCredentialKey('gitlab:abc')).toBeNull()
    expect(fromCredentialKey('jira:')).toBeNull()
  })

  it('lists Jira sites then GitHub tokens, each labelled with its provider', () => {
    const options = credentialOptions(
      [{ id: 'j1', name: 'Work' }],
      [{ id: 'g1', name: 'Personal' }],
      { jira: 'Jira', github: 'GitHub' },
    )
    expect(options).toEqual([
      { id: 'jira:j1', label: 'Jira: Work' },
      { id: 'github:g1', label: 'GitHub: Personal' },
    ])
  })
})

describe('toLinkTarget', () => {
  it('names nothing until a thing is picked', () => {
    expect(toLinkTarget({ ...EMPTY_LINK_TARGET_DRAFT, credentialKey: 'jira:j1' })).toBeNull()
  })

  it('links Jira issues whatever kind the draft last held', () => {
    const target = toLinkTarget({
      ...EMPTY_LINK_TARGET_DRAFT,
      credentialKey: 'jira:j1',
      kind: 'pull-request',
      reference: 'ELY-3',
    })
    expect(target).toEqual({ provider: 'jira', credentialId: 'j1', kind: 'issue', reference: 'ELY-3' })
  })

  it('keeps a GitHub pull request a pull request', () => {
    const target = toLinkTarget({
      ...EMPTY_LINK_TARGET_DRAFT,
      credentialKey: 'github:g1',
      kind: 'pull-request',
      reference: 'acme/elysium#12',
    })
    expect(target).toEqual({
      provider: 'github',
      credentialId: 'g1',
      kind: 'pull-request',
      reference: 'acme/elysium#12',
    })
  })
})

describe('describePendingWrite', () => {
  it('reads an untried write as on its way', () => {
    expect(describePendingWrite(makePendingWrite({ id: 'w1' }))).toEqual({
      labelKey: 'links.pending.close',
      attempts: null,
      lastError: null,
    })
  })

  it('counts tries and keeps the provider’s last answer', () => {
    const write = makePendingWrite({ id: 'w1', kind: 'comment', attempts: 3, lastError: 'rate limited' })
    expect(describePendingWrite(write)).toEqual({
      labelKey: 'links.pending.comment',
      attempts: 3,
      lastError: 'rate limited',
    })
  })
})
