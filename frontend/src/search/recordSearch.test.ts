// Copyright © 2026 Jalapeno Labs

import type { RecordSearchHit } from '../api/routes/searchRoutes'

// Core
import i18next from 'i18next'
import { describe, expect, it, vi } from 'vitest'

// Misc
import { TRANSLATION_NAMESPACES } from '../i18n'
import { toRecordResults } from './recordSearch'
import { scoreSearchMatch, toSearchWords } from './searchMatch'

const t = i18next.getFixedT(null, TRANSLATION_NAMESPACES)

describe('toRecordResults', () => {
  it('opens each kind on its own page, in its own section, with its quieter line', () => {
    const hits: RecordSearchHit[] = [
      { kind: 'actionItem', id: 'i1', title: 'Fix sign-in', state: 'resolved' },
      { kind: 'project', id: 'p1', title: 'Sign-in service' },
      { kind: 'codingSession', id: 42, title: 'Sign-in redirect' },
      { kind: 'initiative', id: 'n1', title: 'Sign-in revamp', state: 'achieved' },
      { kind: 'page', id: 'g1', title: 'Sign-in runbook', pendingSuggestions: 0 },
      { kind: 'page', id: 'g2', title: 'Sign-in glossary', pendingSuggestions: 2 },
    ]

    const results = toRecordResults(hits, 'sign', t)
    expect(results.map((result) => [ result.group, result.target, result.detail ])).toEqual([
      [ 'actionItems', { kind: 'link', href: '/action-items/i1' }, 'Resolved' ],
      [ 'projects', { kind: 'link', href: '/projects/p1' }, undefined ],
      [ 'codingSessions', { kind: 'link', href: '/coding/42' }, '#42' ],
      [ 'initiatives', { kind: 'link', href: '/action-items/initiatives/n1' }, 'Achieved' ],
      [ 'knowledge', { kind: 'link', href: '/knowledge/pages/g1' }, 'Pages' ],
      [ 'knowledge', { kind: 'link', href: '/knowledge/pages/g2' }, 'Pages · 2 suggested edits wait' ],
    ])
    expect(new Set(results.map((result) => result.id)).size).toBe(hits.length)
  })

  it('scores a record by its title as the registry scores a page, so sources rank alike', () => {
    const [ result ] = toRecordResults([{ kind: 'project', id: 'p1', title: 'Platform' }], 'plat', t)
    const pageScore = scoreSearchMatch(toSearchWords('plat'), {
      titleWords: toSearchWords('Platform'),
      tagWords: [],
      parentWords: [],
    })
    expect(result.score).toBe(pageScore)
  })

  it('leaves out, and logs, a kind of record this build does not know', () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {})
    // As the API's answer arrives: parsed JSON, unchecked by the compiler.
    const hits: RecordSearchHit[] = JSON.parse(JSON.stringify([
      { kind: 'meeting', id: 'm1', title: 'Standup' },
      { kind: 'project', id: 'p1', title: 'Standup notes' },
    ]))

    expect(toRecordResults(hits, 'standup', t).map((result) => result.title)).toEqual([ 'Standup notes' ])
    expect(debug).toHaveBeenCalled()
    debug.mockRestore()
  })

  // The API matched it, so it shows even when the browser reads its title differently.
  it('ranks a record the browser would not have matched last, rather than dropping it', () => {
    const results = toRecordResults([{ kind: 'project', id: 'p1', title: 'Nothing alike' }], 'zzz', t)
    expect(results).toHaveLength(1)
    expect(results[0].score).toBe(0)
  })
})
