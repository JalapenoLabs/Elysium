// Copyright © 2026 Jalapeno Labs

import type { SearchActionContext, SearchAudience, SearchViewer } from './searchTypes'

// Core
import i18next from 'i18next'
import { describe, expect, it, vi } from 'vitest'

// Misc
import { TRANSLATION_NAMESPACES } from '../i18n'
import { UrlTree } from '../urls'
import { findRegistryResults, prepareSearchEntries } from './registrySearch'
import { rankSearchResults } from './searchRanking'
import { registeredSearchEntries } from './searchRegistry'

const t = i18next.getFixedT(null, TRANSLATION_NAMESPACES)

const everyone = {
  everyone: true,
  members: true,
  admins: true,
} as const satisfies Record<SearchAudience, boolean>

function search(
  query: string,
  audiences: Record<SearchAudience, boolean> = everyone,
  hasTwoFactor = false,
) {
  const viewer: SearchViewer = {
    audiences,
    conditions: { twoFactorOn: hasTwoFactor, twoFactorOff: !hasTwoFactor },
  }
  const context: SearchActionContext = { dispatch: vi.fn(), confirm: vi.fn() }
  const prepared = prepareSearchEntries(registeredSearchEntries, t, viewer)
  return rankSearchResults(findRegistryResults(prepared, query, context))
}

// The entries every `searchEntries.ts` registers, gathered by Vite as the app gathers them.
describe('registeredSearchEntries', () => {
  it('gathers the entries of every part of the app', () => {
    const ids = registeredSearchEntries.map((entry) => entry.id)
    expect(ids).toEqual(expect.arrayContaining([ 'actionItems', 'security', 'twoFactor', 'signOut', 'openElysia' ]))
  })

  it('names every entry once', () => {
    const ids = registeredSearchEntries.map((entry) => entry.id)
    expect(new Set(ids).size).toBe(ids.length)
  })

  it('links only to pages the app routes, never to a record', () => {
    const paths = new Set<string>(Object.values(UrlTree))
    for (const entry of registeredSearchEntries) {
      if (!('href' in entry)) {
        continue
      }
      const path = entry.href.split('?')[0]
      expect(paths.has(path), `${entry.id} links to ${entry.href}`).toBe(true)
      expect(path, entry.id).not.toContain(':')
    }
  })

  it('reads every title, tag list, and parent from translations that exist', () => {
    for (const entry of registeredSearchEntries) {
      for (const key of [ entry.title, entry.tags, entry.parent ]) {
        if (key) {
          expect(i18next.exists(key), `${entry.id}: ${key}`).toBe(true)
        }
      }
    }
  })
})

// `~/.claude/docs/ux.md`, Global searchbars: hidden tags find what the visible words miss,
// and actions are as searchable as pages.
describe('searching the registry', () => {
  it('finds Two-factor authentication for "2FA", by its hidden tags', () => {
    const page = search('2FA').find((result) => result.group === 'pages')
    expect(page?.title).toBe('Two-factor authentication')
  })

  it('finds Sign out, as an action, for "sign out" and "log out"', () => {
    for (const query of [ 'sign out', 'log out' ]) {
      const first = search(query)[0]
      expect(first?.title).toBe('Sign out')
      expect(first?.group).toBe('actions')
      expect(first?.target.kind).toBe('action')
    }
  })

  it('links to a page with a real address', () => {
    const security = search('security').find((result) => result.title === 'Security')
    expect(security?.target).toEqual({ kind: 'link', href: UrlTree.settingsSecurity })
    expect(security?.detail).toBe('Settings')
  })

  it('opens Security on the tab a passkey is kept on', () => {
    const passkeys = search('passkey')[0]
    expect(passkeys?.target).toEqual({ kind: 'link', href: '/settings/security?tab=passkeys' })
  })

  it('leaves out what the viewer cannot open', () => {
    const guest = { everyone: true, members: false, admins: false }
    const member = { everyone: true, members: true, admins: false }

    expect(search('users', guest).map((result) => result.title)).not.toContain('Users')
    expect(search('runs', guest).map((result) => result.title)).not.toContain('Runs')
    expect(search('runs', member).map((result) => result.title)).toContain('Runs')
    expect(search('users', member).map((result) => result.title)).not.toContain('Users')
    expect(search('users').map((result) => result.title)).toContain('Users')
  })

  // ux.md, 2FA: each has a page of its own, and only the ones that apply are offered.
  it('offers Add 2FA while it is off, and Change and Remove while it is on', () => {
    const whileOff = search('2fa').filter((result) => result.group === 'actions')
    expect(whileOff.map((result) => result.target)).toEqual([
      { kind: 'link', href: UrlTree.settingsTwoFactorNew },
    ])

    const whileOn = search('2fa', everyone, true).filter((result) => result.group === 'actions')
    expect(whileOn.map((result) => result.title).sort()).toEqual([ 'Change 2FA', 'Remove 2FA' ])
    expect(whileOn.map((result) => result.target).sort()).toEqual(expect.arrayContaining([
      { kind: 'link', href: UrlTree.settingsTwoFactorEdit },
      { kind: 'link', href: UrlTree.settingsTwoFactorRemove },
    ]))
  })

  it('finds the 2FA actions by what people call them', () => {
    expect(search('turn on two factor')[0]?.title).toBe('Add 2FA')
    expect(search('remove 2fa', everyone, true)[0]?.title).toBe('Remove 2FA')
  })

  it('runs an action with the context it was found with', () => {
    const context: SearchActionContext = { dispatch: vi.fn(), confirm: vi.fn() }
    const viewer: SearchViewer = {
      audiences: everyone,
      conditions: { twoFactorOn: false, twoFactorOff: true },
    }
    const prepared = prepareSearchEntries(registeredSearchEntries, t, viewer)
    const dark = findRegistryResults(prepared, 'dark theme', context)[0]

    expect(dark?.target.kind).toBe('action')
    if (dark?.target.kind === 'action') {
      dark.target.run()
    }
    expect(context.dispatch).toHaveBeenCalledWith(expect.objectContaining({
      type: 'theme/themeChanged',
      payload: expect.objectContaining({ preference: 'dark' }),
    }))
  })
})
