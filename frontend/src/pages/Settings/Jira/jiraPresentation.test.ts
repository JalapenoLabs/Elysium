// Copyright © 2026 Jalapeno Labs

import type { JiraCredential } from '../../../api/routes/jiraRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { ALL_JIRA_ITEMS } from '../../../api/routes/jiraRoutes'
import {
  JIRA_SITE_URL_PATTERN,
  findMissingSelectionNames,
  keepsStoredJiraToken,
  normalizeJiraSiteUrl,
  summarizeScope,
  toJiraScopeSelection,
} from './jiraPresentation'

function makeCredential(overrides: Partial<JiraCredential> = {}): JiraCredential {
  return {
    id: '0199',
    name: 'Work',
    siteUrl: 'https://acme.atlassian.net',
    accountEmail: 'alex@example.com',
    accountId: '5b10',
    displayName: 'Alex Navarro',
    projects: [{ id: '10002', key: 'ELY', name: 'Elysium' }],
    boards: [{ id: 12, name: 'ELY board', projectKey: 'ELY' }],
    checkedAt: '2026-09-18T12:00:00.000Z',
    createdAt: '2026-09-18T12:00:00.000Z',
    updatedAt: '2026-09-18T12:00:00.000Z',
    ...overrides,
  }
}

describe('JIRA_SITE_URL_PATTERN', () => {
  it('accepts the sites the API and the database column accept', () => {
    // The pattern runs on a normalized value, so it only ever sees this spelling.
    for (const siteUrl of [
      'https://acme.atlassian.net',
      'https://acme.jira-dev.atlassian.net',
      'https://a1.atlassian.net',
    ]) {
      expect(JIRA_SITE_URL_PATTERN.test(siteUrl), siteUrl).toBe(true)
    }
  })

  it('refuses what they refuse, including a label starting with a hyphen', () => {
    for (const siteUrl of [
      'http://acme.atlassian.net',
      'https://atlassian.net',
      'https://acme.atlassian.net.evil.com',
      'https://jira.example.com',
      'https://acme.atlassian.net:8443',
      'https://acme.atlassian.net/jira',
      // The column's constraint starts every label with a letter or a digit.
      'https://-acme.atlassian.net',
      'https://acme..atlassian.net',
      'https://acme_test.atlassian.net',
      'acme.atlassian.net',
      '',
    ]) {
      expect(JIRA_SITE_URL_PATTERN.test(siteUrl), siteUrl).toBe(false)
    }
  })

  it('accepts what a person pastes, once it is normalized', () => {
    expect(normalizeJiraSiteUrl('  https://ACME.Atlassian.net/  ')).toBe('https://acme.atlassian.net')
    expect(JIRA_SITE_URL_PATTERN.test(normalizeJiraSiteUrl('https://ACME.atlassian.net/'))).toBe(true)
  })
})

describe('keepsStoredJiraToken', () => {
  it('keeps the stored token only while its site and account are unchanged', () => {
    const stored = makeCredential()

    expect(keepsStoredJiraToken(stored, 'https://ACME.atlassian.net/', ' alex@example.com ')).toBe(true)
    expect(keepsStoredJiraToken(stored, 'https://other.atlassian.net', 'alex@example.com')).toBe(false)
    expect(keepsStoredJiraToken(stored, 'https://acme.atlassian.net', 'sam@example.com')).toBe(false)
    expect(keepsStoredJiraToken(null, 'https://acme.atlassian.net', 'alex@example.com')).toBe(false)
  })
})

describe('toJiraScopeSelection', () => {
  it('reads a stored allowlist as the switches and ids a form holds', () => {
    expect(toJiraScopeSelection(makeCredential())).toEqual({
      allProjects: false,
      projectIds: [ '10002' ],
      allBoards: false,
      boardIds: [ 12 ],
    })

    const everything = makeCredential({ projects: ALL_JIRA_ITEMS, boards: ALL_JIRA_ITEMS })
    expect(toJiraScopeSelection(everything)).toEqual({
      allProjects: true,
      projectIds: [],
      allBoards: true,
      boardIds: [],
    })
  })
})

describe('findMissingSelectionNames', () => {
  it('answers per list, so one listing says nothing about the other', () => {
    const stored = makeCredential({
      projects: [
        { id: '10002', key: 'ELY', name: 'Elysium' },
        { id: '10003', key: 'OPS', name: 'Operations' },
      ],
      boards: [
        { id: 12, name: 'ELY board', projectKey: 'ELY' },
        { id: 13, name: 'OPS board', projectKey: 'OPS' },
      ],
    })

    const missing = findMissingSelectionNames(
      stored,
      [{ id: '10002', key: 'ELY', name: 'Elysium' }],
      [{ id: 13, name: 'OPS board', projectKey: 'OPS' }],
    )

    expect(missing.projects).toEqual([ 'OPS' ])
    expect(missing.boards).toEqual([ 'ELY board' ])
  })

  it('finds nothing for an allowlist of everything, which names nothing in particular', () => {
    const everything = makeCredential({ projects: ALL_JIRA_ITEMS, boards: ALL_JIRA_ITEMS })

    expect(findMissingSelectionNames(everything, [], [])).toEqual({ projects: [], boards: []})
  })
})

describe('summarizeScope', () => {
  it('names the first few and counts the rest', () => {
    expect(summarizeScope([ 'ELY', 'OPS', 'FIN', 'MKT', 'SEC' ])).toEqual({
      count: 5,
      preview: [ 'ELY', 'OPS', 'FIN' ],
      remaining: 2,
    })
    expect(summarizeScope([])).toEqual({ count: 0, preview: [], remaining: 0 })
  })
})
