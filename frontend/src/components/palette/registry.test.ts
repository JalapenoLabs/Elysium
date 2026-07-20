// Copyright © 2026 Jalapeno Labs

import type { PaletteEntry } from './registry'

// Core
import { describe, expect, it, vi } from 'vitest'

// Misc
import {
  listPaletteEntries,
  rankPaletteEntries,
  registerPaletteEntries,
  subscribePalette,
} from './registry'

function makeEntry(id: string, label: string, keywords?: string): PaletteEntry {
  return {
    id,
    label,
    keywords,
    run: () => {},
  }
}

describe('registerPaletteEntries', () => {
  it('adds a group to the snapshot and removes it on unregister', () => {
    const entries = [ makeEntry('one', 'One') ]
    const unregister = registerPaletteEntries('test.group', entries)

    expect(listPaletteEntries()).toContain(entries[0])

    unregister()
    expect(listPaletteEntries()).not.toContain(entries[0])
  })

  it('lets a re-registration survive the previous registration unregistering late', () => {
    const staleEntries = [ makeEntry('stale', 'Stale') ]
    const freshEntries = [ makeEntry('fresh', 'Fresh') ]

    const unregisterStale = registerPaletteEntries('test.group', staleEntries)
    const unregisterFresh = registerPaletteEntries('test.group', freshEntries)

    // The stale unregister fires after the group was replaced (a React strict
    // effect ordering); it must not wipe the fresh entries.
    unregisterStale()
    expect(listPaletteEntries()).toContain(freshEntries[0])

    unregisterFresh()
    expect(listPaletteEntries()).not.toContain(freshEntries[0])
  })

  it('notifies subscribers on registration changes and stops after unsubscribe', () => {
    const listener = vi.fn()
    const unsubscribe = subscribePalette(listener)

    const unregister = registerPaletteEntries('test.notify', [ makeEntry('a', 'A') ])
    expect(listener).toHaveBeenCalledTimes(1)

    unregister()
    expect(listener).toHaveBeenCalledTimes(2)

    unsubscribe()
    const unregisterAgain = registerPaletteEntries('test.notify', [ makeEntry('b', 'B') ])
    unregisterAgain()
    expect(listener).toHaveBeenCalledTimes(2)
  })

  it('returns a stable snapshot reference between changes', () => {
    const unregister = registerPaletteEntries('test.stable', [ makeEntry('a', 'A') ])
    expect(listPaletteEntries()).toBe(listPaletteEntries())
    unregister()
  })
})

describe('rankPaletteEntries', () => {
  const settings = makeEntry('settings', 'Personal settings', 'profile security')
  const organization = makeEntry('organization', 'Organization settings')
  const home = makeEntry('home', 'Home')
  const all = [ home, settings, organization ]

  it('returns every entry unranked for an empty query', () => {
    const ranked = rankPaletteEntries('   ', all)
    expect(ranked.map((result) => result.entry.id)).toEqual([ 'home', 'settings', 'organization' ])
    expect(ranked.every((result) => result.labelIndices.length === 0)).toBe(true)
  })

  it('excludes entries that match neither label nor keywords', () => {
    const ranked = rankPaletteEntries('zzzz', all)
    expect(ranked).toEqual([])
  })

  it('matches through keywords and still ranks label matches with indices', () => {
    const ranked = rankPaletteEntries('security', all)
    expect(ranked.map((result) => result.entry.id)).toContain('settings')
  })

  it('ranks the closer label match first', () => {
    const ranked = rankPaletteEntries('home', all)
    expect(ranked[0].entry.id).toBe('home')
    expect(ranked[0].labelIndices.length).toBe(4)
  })
})
