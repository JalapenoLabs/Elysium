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
    ])

    expect(rows.map((row) => [ row.entry.id, row.sentence, row.canUndo ])).toEqual([
      [ 'undo', 'undidEdit', true ],
      [ 'edit', 'edited', false ],
      [ 'hold', 'held', true ],
      [ 'built', 'builtFromCorrection', false ],
      [ 'later', 'changed', false ],
    ])
    expect([ rows[1].before, rows[1].after ]).toEqual([ 'Fryday.', 'Friday.' ])
  })

  it('says when the correction settled the transcript, which only an edit changes', () => {
    const [ settled ] = describeHistory([
      entry('settled', 'settled', { changes: [], marksAdded: [ 'mark' ], skipped: 0 }),
    ])

    expect([ settled.sentence, settled.canUndo ]).toEqual([ 'settled', false ])
  })

  it('says when a recording was moved on the clock, merged in, or split out', () => {
    const rows = describeHistory([
      entry('offset', 'offset_changed', { captureId: 'capture' }),
      entry('merged', 'merged', { from: 'other', into: 'this' }),
      entry('split', 'split', { from: 'other', into: 'this' }),
    ])

    // An offset set by hand is undone from the list; a merge is undone by splitting it.
    expect(rows.map((row) => [ row.sentence, row.canUndo ])).toEqual([
      [ 'offsetChanged', true ],
      [ 'merged', false ],
      [ 'split', false ],
    ])
  })

  it('names the kind of a mark made or removed, and shortens a long quote', () => {
    const [ marked, edited ] = describeHistory([
      entry('marked', 'marked', { kind: 'black_out' }),
      entry('edited', 'edited', { beforeText: 'word '.repeat(40), afterText: 'short' }),
    ])

    expect(marked.markKind).toBe('black_out')
    expect(edited.before?.endsWith('…')).toBe(true)
    expect(edited.before?.length).toBeLessThanOrEqual(60)
  })

  it('says when segmenting took excerpts and black-outs, or a review was done, neither undone whole', () => {
    const rows = describeHistory([
      entry('again', 'review_done'),
      entry('took', 'segmented', { made: [ 'excerpt' ], kept: [], removed: [], unanchored: 0 }),
      entry('excerpt', 'marked', { kind: 'excerpt', recordId: 'record' }),
      entry('lifted', 'unmarked', { kind: 'black_out', restored: []}),
    ])

    expect(rows.map((row) => [ row.sentence, row.markKind, row.canUndo ])).toEqual([
      [ 'reviewDone', null, false ],
      [ 'segmented', null, false ],
      [ 'marked', 'excerpt', true ],
      [ 'unmarked', 'black_out', true ],
    ])
  })

  it('says what an added capture holds, which cannot be undone', () => {
    const rows = describeHistory([
      entry('both', 'capture_added', { captureId: 'capture', recording: true, transcript: true }),
      entry('recording', 'capture_added', { recording: true, transcript: false }),
      entry('transcript', 'capture_added', { recording: false, transcript: true }),
    ])

    expect(rows.map((row) => [ row.sentence, row.canUndo ])).toEqual([
      [ 'addedRecordingAndTranscript', false ],
      [ 'addedRecording', false ],
      [ 'addedTranscript', false ],
    ])
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
