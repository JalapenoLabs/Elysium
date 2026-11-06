// Copyright © 2026 Jalapeno Labs

import type { SearchGroup, SearchResult } from './searchTypes'

// Core
import { describe, expect, it } from 'vitest'

// User interface
import { LuSearch } from 'react-icons/lu'

// Misc
import { groupSearchResults, mergeSearchResults, rankSearchResults } from './searchRanking'

function result(id: string, group: SearchGroup, score: number, title = id): SearchResult {
  return {
    id,
    group,
    title,
    icon: LuSearch,
    score,
    target: { kind: 'link', href: `/${id}` },
  }
}

describe('mergeSearchResults', () => {
  it('shows a thing two sources found once, as the later source found it', () => {
    const byTitle = { ...result('page:p1', 'knowledge', 3), detail: 'Page' }
    const byText = { ...result('page:p1', 'knowledge', 3), detail: 'Page · Maya Chen' }
    const answer = { isLoading: false, hasFailed: false }

    const merged = mergeSearchResults([
      { ...answer, results: [ byTitle, result('settings', 'pages', 1) ]},
      { ...answer, results: [ byText ]},
    ])

    expect(merged.map((entry) => entry.id)).toEqual([ 'page:p1', 'settings' ])
    expect(merged[0].detail).toBe('Page · Maya Chen')
  })
})

describe('rankSearchResults', () => {
  it('puts the closest match first, then equal matches by title', () => {
    const ranked = rankSearchResults([
      result('b', 'pages', 3, 'Beta'),
      result('a', 'pages', 3, 'Alpha'),
      result('c', 'actions', 7, 'Gamma'),
    ])

    expect(ranked.map((entry) => entry.id)).toEqual([ 'c', 'a', 'b' ])
  })

  it('leaves the results it was given in their order', () => {
    const results = [ result('a', 'pages', 1), result('b', 'pages', 2) ]
    rankSearchResults(results)
    expect(results.map((entry) => entry.id)).toEqual([ 'a', 'b' ])
  })
})

describe('groupSearchResults', () => {
  it('places each section where its best result ranks, keeping the order inside it', () => {
    const sections = groupSearchResults([
      result('signOut', 'actions', 9),
      result('security', 'pages', 8),
      result('newItem', 'actions', 7),
      result('settings', 'pages', 6),
    ])

    expect(sections.map((section) => section.group)).toEqual([ 'actions', 'pages' ])
    expect(sections[0].results.map((entry) => entry.id)).toEqual([ 'signOut', 'newItem' ])
    expect(sections[1].results.map((entry) => entry.id)).toEqual([ 'security', 'settings' ])
  })

  it('has no sections without results', () => {
    expect(groupSearchResults([])).toEqual([])
  })
})
