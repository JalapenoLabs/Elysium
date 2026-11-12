// Copyright © 2026 Jalapeno Labs

import type { KnowledgeResult } from '../api/routes/knowledgeRoutes'

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { toKnowledgeResults } from './knowledgeSearch'
import { rankSearchResults } from './searchRanking'

function result(
  documentId: string,
  title: string,
  source = 'slack',
  pageId: string | null = null,
): KnowledgeResult {
  return {
    citation: {
      documentId,
      source,
      title,
      url: null,
      pageId,
      meetingId: null,
      occurredAt: '2026-11-01T10:00:00Z',
      person: { id: 'u1', name: 'Maya Chen' },
    },
    passages: [
      { position: 2, content: 'The sign-in loop on Safari.' },
      { position: 0, content: 'WAY-123.' },
    ],
    alsoRecordedBy: [],
    score: 0.03,
    pendingSuggestions: 0,
  }
}

const t = i18next.getFixedT('en-US', 'knowledge')

describe('toKnowledgeResults', () => {
  it('opens each document at its best passage, naming its source and person', () => {
    const [ found ] = toKnowledgeResults([ result('d1', '#web') ], 'safari', t)

    expect(found.id).toBe('knowledge:d1')
    expect(found.group).toBe('knowledge')
    expect(found.title).toBe('#web')
    expect(found.detail).toBe('Slack · Maya Chen')
    expect(found.target).toEqual({ kind: 'link', href: '/knowledge/documents/d1?passage=2' })
  })

  it('opens what was indexed from a page in the page, under the id the record search gives it', () => {
    const [ found ] = toKnowledgeResults([ result('d1', 'Runbook', 'page', 'p1') ], 'safari', t)

    expect(found.id).toBe('page:p1')
    expect(found.target).toEqual({ kind: 'link', href: '/knowledge/pages/p1?passage=2' })
  })

  it('says how many suggested edits wait on a page it found', () => {
    const [ found ] = toKnowledgeResults(
      [{ ...result('d1', 'Glossary', 'company_doc', 'p1'), pendingSuggestions: 3 }],
      'acme',
      t,
    )

    expect(found.detail).toBe('Company doc · Maya Chen · 3 suggested edits wait')
  })

  it('names a document without a title by its source', () => {
    const [ found ] = toKnowledgeResults([ result('d1', '', 'meeting') ], 'plans', t)

    expect(found.title).toBe('Meetings')
  })

  it('scores a title that matches above one found by what it says, keeping the search\'s order otherwise', () => {
    const found = toKnowledgeResults([
      result('d1', '#web'),
      result('d2', '#ops'),
      result('d3', 'Safari sign-in'),
    ], 'safari', t)

    const ranked = rankSearchResults(found).map((each) => each.id)

    expect(ranked).toEqual([ 'knowledge:d3', 'knowledge:d1', 'knowledge:d2' ])
  })
})
