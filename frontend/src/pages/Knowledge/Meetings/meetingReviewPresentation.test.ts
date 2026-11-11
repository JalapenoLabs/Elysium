// Copyright © 2026 Jalapeno Labs

import type { MeetingHistoryEntry } from '../../../api/routes/meetingRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeUser } from '../../../testFixtures'
import { authorOf, describeHistory, entryOfEditMark } from './meetingReviewPresentation'

function entry(id: string, kind: string, data: Record<string, unknown> = {}, undid: string | null = null) {
  return {
    id,
    kind,
    createdBy: 'person',
    agent: null,
    data,
    undid,
    createdAt: '2026-11-10T12:00:00Z',
  } satisfies MeetingHistoryEntry
}

describe('describeHistory', () => {
  it('says what each change did, offering Undo only where it still applies', () => {
    const rows = describeHistory([
      entry('undo', 'edited', { beforeText: 'Friday.', afterText: 'Fryday.' }, 'edit'),
      entry('edit', 'edited', { beforeText: 'Fryday.', afterText: 'Friday.', marksAdded: [ 'mark' ]}),
      entry('hold', 'hold_changed', { before: false, after: true }),
      entry('built', 'built', { builtFrom: 'correction' }),
      entry('later', 'spliced'),
    ], 'this')

    expect(rows.map((row) => [ row.entry.id, row.sentence, row.canUndo ])).toEqual([
      [ 'undo', 'undidEdit', true ],
      [ 'edit', 'edited', false ],
      [ 'hold', 'held', true ],
      [ 'built', 'builtFromCorrection', false ],
      [ 'later', 'changed', false ],
    ])
    expect([ rows[1].before, rows[1].after ]).toEqual([ 'Fryday.', 'Friday.' ])
  })

  it('says when a recording was moved on the clock, and when that was undone', () => {
    const rows = describeHistory([
      entry('back', 'offset_changed', { captureId: 'capture' }, 'moved'),
      entry('moved', 'offset_changed', { captureId: 'capture' }),
    ], 'this')

    expect(rows.map((row) => [ row.sentence, row.canUndo ])).toEqual([
      [ 'undidOffset', true ],
      [ 'offsetChanged', false ],
    ])
  })

  it('reads a merge and a split from the side of the meeting it is listed on', () => {
    const merge = { from: 'other', into: 'this' }
    const entries = [
      entry('mergedAgain', 'merged', merge, 'split'),
      entry('split', 'split', merge, 'merged'),
      entry('merged', 'merged', merge),
    ]

    // The meeting the recordings went into undoes either: the newest merge splits them out again.
    const kept = describeHistory(entries, 'this')
    expect(kept.map((row) => [ row.sentence, row.canUndo ])).toEqual([
      [ 'mergedIn', true ],
      [ 'splitOut', false ],
      [ 'mergedIn', false ],
    ])

    // The meeting they came from records both, and undoes neither.
    const away = describeHistory(entries, 'other')
    expect(away.map((row) => [ row.sentence, row.canUndo ])).toEqual([
      [ 'mergedAway', false ],
      [ 'splitAway', false ],
      [ 'mergedAway', false ],
    ])

    // An entry naming neither side reads as a change, with nothing to undo.
    const [ unreadable ] = describeHistory([ entry('odd', 'merged', { from: 'a', into: 'b' }) ], 'this')
    expect([ unreadable.sentence, unreadable.canUndo ]).toEqual([ 'changed', false ])
  })

  it('names the kind of a mark made or removed, and shortens a long quote', () => {
    const [ marked, edited ] = describeHistory([
      entry('marked', 'marked', { kind: 'black_out' }),
      entry('edited', 'edited', { beforeText: 'word '.repeat(40), afterText: 'short' }),
    ], 'this')

    expect(marked.markKind).toBe('black_out')
    expect(edited.before?.endsWith('…')).toBe(true)
    expect(edited.before?.length).toBeLessThanOrEqual(60)
  })
})

describe('entryOfEditMark', () => {
  it('finds the newest change that stood an edit\'s mark', () => {
    const entries = [
      entry('redo', 'edited', { marksAdded: [ 'mark' ]}, 'undo'),
      entry('undo', 'edited', { marksRemoved: [ 'mark' ]}, 'edit'),
      entry('edit', 'edited', { marksAdded: [ 'mark' ]}),
    ]

    expect(entryOfEditMark(entries, 'mark')?.id).toBe('redo')
    expect(entryOfEditMark(entries, 'other')).toBeNull()
  })
})

describe('authorOf', () => {
  const usersById = {
    person: makeUser({ id: 'person', name: 'Ada Lovelace' }),
    elysia: makeUser({ id: 'elysia', name: 'Elysia', kind: 'machine', role: 'agent' }),
  }

  it('names a person, and says an agent is one', () => {
    expect(authorOf('person', null, usersById)).toEqual({ name: 'Ada Lovelace', isAgent: false })
    expect(authorOf('elysia', null, usersById)).toEqual({ name: 'Elysia', isAgent: true })
    expect(authorOf('person', { id: 'mcp:claude', name: 'Claude Code' }, usersById))
      .toEqual({ name: 'Claude Code', isAgent: true })
    expect(authorOf('gone', null, usersById)).toEqual({ name: null, isAgent: false })
  })
})
