// Copyright © 2026 Jalapeno Labs

import type { CompanyDocHistoryEntry } from '../../../api/routes/companyDocRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { describeAgent, describeHistoryEntry } from './companyDocPresentation'

function entry(kind: string, data: Record<string, unknown>): CompanyDocHistoryEntry {
  return {
    id: 'e1',
    kind,
    createdBy: 'u1',
    proposalId: null,
    data,
    createdAt: '2026-11-04T10:00:00Z',
  }
}

describe('describeHistoryEntry', () => {
  it('names the sections a change wrote, in the order the doc is read', () => {
    const described = describeHistoryEntry(entry('edited', {
      changes: {
        glossary: { from: '', to: '- ACME' },
        people: { from: '', to: '- Priya Raman' },
      },
    }))

    expect(described).toEqual({ kind: 'edited', sections: [ 'people', 'glossary' ], undid: false })
  })

  it('says when an undo wrote it', () => {
    const described = describeHistoryEntry(entry('edited', {
      changes: { people: { from: '- Priya', to: '' }},
      undid: '0199a3c4-0000-7000-8000-000000000001',
    }))

    expect(described.undid).toBe(true)
  })

  it('still reads an entry of a kind or shape it does not know', () => {
    expect(describeHistoryEntry(entry('renamed', { changes: 'odd' }))).toEqual({
      kind: null,
      sections: [],
      undid: false,
    })
  })
})

describe('describeAgent', () => {
  it('names a session by its number and a client by the name it registered', () => {
    expect(describeAgent({ id: 'session:12', name: null })).toEqual({ kind: 'session', number: '12' })
    expect(describeAgent({ id: 'mcp:abc', name: 'Claude Code' })).toEqual({ kind: 'client', name: 'Claude Code' })
  })
})
