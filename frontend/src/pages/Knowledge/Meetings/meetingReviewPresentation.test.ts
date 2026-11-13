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

  it('says who reads it and whom it was shared with, the correction\'s decision never undone', () => {
    const rows = describeHistory([
      entry('unshared', 'unshared', { personId: 'grace' }),
      entry('shared', 'shared', { personId: 'grace' }),
      entry('opened', 'visibility_changed', { before: 'private', after: 'public' }),
      entry('back', 'visibility_changed', { before: 'public', after: 'unchecked' }),
      entry('kept', 'visibility_changed', { after: 'private', correction: true, why: 'A review.' }),
      entry('unsure', 'visibility_changed', { after: 'private', correction: true, undecided: true }),
      entry('open', 'visibility_changed', { after: 'public', correction: true }),
    ], 'this')

    expect(rows.map((row) => [ row.entry.id, row.sentence, row.canUndo, row.personId ])).toEqual([
      [ 'unshared', 'unshared', true, 'grace' ],
      [ 'shared', 'shared', true, 'grace' ],
      [ 'opened', 'opened', true, null ],
      [ 'back', 'leftToCorrection', true, null ],
      [ 'kept', 'keptPrivateByCorrection', false, null ],
      [ 'unsure', 'keptPrivateUndecided', false, null ],
      [ 'open', 'openedByCorrection', false, null ],
    ])
  })

  it('says when the correction settled the transcript, which only an edit changes', () => {
    const [ settled ] = describeHistory([
      entry('settled', 'settled', { changes: [], marksAdded: [ 'mark' ], skipped: 0 }),
    ], 'this')

    expect([ settled.sentence, settled.canUndo ]).toEqual([ 'settled', false ])
  })

  it('says who deleted and restored the meeting, offering Undo on neither', () => {
    const rows = describeHistory([
      entry('restored', 'restored', { deletedAt: '2026-11-11T09:00:00Z', excerpts: 2, runRetried: true }),
      entry('removed', 'deleted', { excerpts: 2, connectorId: 'fireflies' }),
      entry('deleted', 'deleted', { excerpts: 2, connectorId: null }),
    ], 'this')

    // Restoring is a deletion's undo, and deleting again a restore's.
    expect(rows.map((row) => [ row.sentence, row.canUndo ])).toEqual([
      [ 'restored', false ],
      [ 'deletedWithConnector', false ],
      [ 'deleted', false ],
    ])
  })

  it('names a file attached or taken off, and reads a pin as pinning rather than a mark', () => {
    const rows = describeHistory([
      entry('putBack', 'attached', { attachmentKind: 'file', fileName: 'Roadmap.pdf' }, 'detached'),
      entry('detached', 'detached', { attachmentKind: 'file', fileName: 'Roadmap.pdf' }),
      entry('frame', 'attached', { attachmentKind: 'frame', fileName: 'Frame at 12m04s.webp' }),
      entry('unpinned', 'unmarked', { kind: 'pin' }),
      entry('pinned', 'marked', { kind: 'pin' }),
      entry('excerpt', 'marked', { kind: 'excerpt' }),
    ], 'this')

    expect(rows.map((row) => [ row.sentence, row.after, row.markKind, row.canUndo ])).toEqual([
      [ 'attachedFile', 'Roadmap.pdf', null, true ],
      [ 'detachedFile', 'Roadmap.pdf', null, false ],
      [ 'attachedFrame', 'Frame at 12m04s.webp', null, true ],
      [ 'unpinned', null, null, true ],
      [ 'pinned', null, null, true ],
      [ 'marked', null, 'excerpt', true ],
    ])
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

  it('says when segmenting took excerpts and black-outs, or a review was done, neither undone whole', () => {
    const rows = describeHistory([
      entry('again', 'review_done'),
      entry('took', 'segmented', { made: [ 'excerpt' ], kept: [], removed: [], unanchored: 0 }),
      entry('excerpt', 'marked', { kind: 'excerpt', recordId: 'record' }),
      entry('lifted', 'unmarked', { kind: 'black_out', restored: []}),
    ], 'this')

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
    ], 'this')

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
