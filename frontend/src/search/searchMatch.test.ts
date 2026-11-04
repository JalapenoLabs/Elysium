// Copyright © 2026 Jalapeno Labs

import type { SearchableText } from './searchMatch'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { scoreSearchMatch, toSearchWords } from './searchMatch'

function searchable(title: string, tags = '', parent = ''): SearchableText {
  return {
    titleWords: toSearchWords(title),
    tagWords: toSearchWords(tags),
    parentWords: toSearchWords(parent),
  }
}

describe('toSearchWords', () => {
  it('lowercases, drops accents, and breaks on punctuation', () => {
    expect(toSearchWords('Two-factor authentication')).toEqual([ 'two', 'factor', 'authentication' ])
    expect(toSearchWords('  Réglages, “MCP”  ')).toEqual([ 'reglages', 'mcp' ])
  })

  it('keeps digits inside words, so 2FA stays one word', () => {
    expect(toSearchWords('2FA / MFA')).toEqual([ '2fa', 'mfa' ])
  })

  it('finds no words in blank text', () => {
    expect(toSearchWords('   ')).toEqual([])
  })
})

describe('scoreSearchMatch', () => {
  it('matches nothing for an empty query', () => {
    expect(scoreSearchMatch([], searchable('Settings'))).toBeNull()
  })

  it('needs every typed word to begin some word of the entry', () => {
    const security = searchable('Security', 'login sign in', 'Settings')

    expect(scoreSearchMatch(toSearchWords('sec'), security)).not.toBeNull()
    expect(scoreSearchMatch(toSearchWords('sign settings'), security)).not.toBeNull()
    expect(scoreSearchMatch(toSearchWords('sign billing'), security)).toBeNull()
    // Inside a word is not the start of one.
    expect(scoreSearchMatch(toSearchWords('curity'), security)).toBeNull()
  })

  it('finds an entry by its hidden tags', () => {
    const twoFactor = searchable('Two-factor authentication', '2fa mfa authenticator')

    expect(scoreSearchMatch(toSearchWords('2FA'), twoFactor)).not.toBeNull()
    expect(scoreSearchMatch(toSearchWords('two factor'), twoFactor)).not.toBeNull()
  })

  it('ranks the title over the tags, and both over the parent page', () => {
    const query = toSearchWords('settings')
    const directory = scoreSearchMatch(query, searchable('Settings'))!
    const tagged = scoreSearchMatch(query, searchable('Preferences', 'settings'))!
    const inside = scoreSearchMatch(query, searchable('Email', '', 'Settings'))!

    expect(directory).toBeGreaterThan(tagged)
    expect(tagged).toBeGreaterThan(inside)
  })

  it('ranks a whole word over the start of one', () => {
    const query = toSearchWords('mail')
    expect(scoreSearchMatch(query, searchable('Mail')))
      .toBeGreaterThan(scoreSearchMatch(query, searchable('Mailboxes'))!)
  })

  it('ranks a title that begins with what was typed over one that holds the same words', () => {
    const query = toSearchWords('new project')
    const begins = scoreSearchMatch(query, searchable('New project'))!
    const holds = scoreSearchMatch(query, searchable('Project overview', 'new'))!

    expect(begins).toBeGreaterThan(holds)
  })
})
