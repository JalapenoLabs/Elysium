// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { LearningAgent } from '../../../api/routes/learningRoutes'
import type { MeetingHistoryEntry } from '../../../api/routes/meetingRoutes'
import type { User } from '../../../api/routes/userRoutes'
import type { DrawnMarkKind } from './meetingTranscript'

// Misc
import { DRAWN_MARK_KINDS } from './meetingTranscript'

// How a meeting's page names its marks, who made them, and each change in its history.

export const markKindLabelKeys = {
  confidence: 'review.marks.confidence',
  edit: 'review.marks.edit',
  excerpt: 'review.marks.excerpt',
  black_out: 'review.marks.blackOut',
  comment: 'review.marks.comment',
} as const satisfies Record<DrawnMarkKind, ParseKeys<'meetings'>>

// Who made a mark or a change: a person by name, or an agent, which says it is one
// (docs/meeting-review.md, Marks). An agent is one acting for a person, or one of Elysium's own
// machine users, such as Elysia.
export type Author = {
  name: string | null
  isAgent: boolean
}

export function authorOf(
  createdBy: string,
  agent: LearningAgent | null,
  usersById: Record<string, User | undefined>,
): Author {
  const user = usersById[createdBy]
  if (agent) {
    return { name: agent.name, isAgent: true }
  }
  return {
    name: user?.name ?? null,
    isAgent: user?.kind === 'machine',
  }
}

export const historySentenceKeys = {
  builtFromCorrection: 'review.history.sentences.builtFromCorrection',
  builtFromSource: 'review.history.sentences.builtFromSource',
  builtFromWhisper: 'review.history.sentences.builtFromWhisper',
  settled: 'review.history.sentences.settled',
  edited: 'review.history.sentences.edited',
  undidEdit: 'review.history.sentences.undidEdit',
  marked: 'review.history.sentences.marked',
  unmarked: 'review.history.sentences.unmarked',
  held: 'review.history.sentences.held',
  released: 'review.history.sentences.released',
  approved: 'review.history.sentences.approved',
  offsetChanged: 'review.history.sentences.offsetChanged',
  undidOffset: 'review.history.sentences.undidOffset',
  mergedIn: 'review.history.sentences.mergedIn',
  mergedAway: 'review.history.sentences.mergedAway',
  splitOut: 'review.history.sentences.splitOut',
  splitAway: 'review.history.sentences.splitAway',
  addedRecordingAndTranscript: 'review.history.sentences.addedRecordingAndTranscript',
  addedRecording: 'review.history.sentences.addedRecording',
  addedTranscript: 'review.history.sentences.addedTranscript',
  threadResolved: 'review.history.sentences.threadResolved',
  threadReopened: 'review.history.sentences.threadReopened',
  opened: 'review.history.sentences.opened',
  madePrivate: 'review.history.sentences.madePrivate',
  leftToCorrection: 'review.history.sentences.leftToCorrection',
  openedByCorrection: 'review.history.sentences.openedByCorrection',
  keptPrivateByCorrection: 'review.history.sentences.keptPrivateByCorrection',
  keptPrivateUndecided: 'review.history.sentences.keptPrivateUndecided',
  shared: 'review.history.sentences.shared',
  unshared: 'review.history.sentences.unshared',
  changed: 'review.history.sentences.changed',
} as const satisfies Record<string, ParseKeys<'meetings'>>

type HistorySentence = keyof typeof historySentenceKeys

export type HistoryRow = {
  entry: MeetingHistoryEntry
  sentence: HistorySentence
  // What it said before and after, for an edit; the kind of mark, for a mark.
  before: string | null
  after: string | null
  markKind: DrawnMarkKind | null
  // Whom a share or an unshare was about, by user id.
  personId: string | null
  // Whether Undo applies: a change that can be undone and was not undone already.
  canUndo: boolean
}

// Long enough to recognize a phrase, short enough for a narrow lane.
const QUOTE_MAX_CHARACTERS = 60

// Kinds undone wherever they are listed. A merge and a split are undone only from the meeting the
// recordings went into (`sideOf`).
const UNDOABLE_KINDS = new Set([
  'edited',
  'marked',
  'unmarked',
  'hold_changed',
  'offset_changed',
  'thread_resolved',
  'thread_reopened',
  'visibility_changed',
  'shared',
  'unshared',
])

// How a change to who reads a meeting reads: a person's, by what it became, and its correction's,
// by what it decided, which nothing undoes since nothing would decide again.
function visibilitySentence(data: Record<string, unknown>): HistorySentence {
  if (data.correction === true) {
    if (data.after === 'public') {
      return 'openedByCorrection'
    }
    return data.undecided === true
      ? 'keptPrivateUndecided'
      : 'keptPrivateByCorrection'
  }
  if (data.after === 'public') {
    return 'opened'
  }
  if (data.after === 'private') {
    return 'madePrivate'
  }
  return 'leftToCorrection'
}

// Which side of a merge or split a meeting's entry was written for (`Moved` in
// api/src/meetings/duplicates.rs): the meeting the recordings went into, or the one they came from.
// Both meetings record it, and it is undone from the kept one alone: undoing a merge there splits
// the recordings back out, and undoing a split merges them again. The other meeting's copy is a
// record of it, which the API refuses to undo.
function sideOf(entry: MeetingHistoryEntry, meetingId: string) {
  if (entry.data.into === meetingId) {
    return 'kept'
  }
  if (entry.data.from === meetingId) {
    return 'away'
  }
  console.debug('describeHistory read a merge or split that names neither side as this meeting', {
    entryId: entry.id,
    meetingId,
  })
  return null
}

const movedSentences = {
  merged: { kept: 'mergedIn', away: 'mergedAway' },
  split: { kept: 'splitOut', away: 'splitAway' },
} as const satisfies Record<'merged' | 'split', Record<'kept' | 'away', HistorySentence>>

// What a transcript was built from (`BuiltFrom` in api/src/models/meeting_word.rs), and how its
// build reads. A build from a corrected copy's lines says `correction`.
const builtSentences = [
  [ 'correction', 'builtFromCorrection' ],
  [ 'source', 'builtFromSource' ],
  [ 'whisper', 'builtFromWhisper' ],
] as const satisfies readonly (readonly [ string, HistorySentence ])[]

function quoted(value: unknown) {
  if (typeof value !== 'string') {
    return null
  }
  const text = value.trim()
  if (text.length <= QUOTE_MAX_CHARACTERS) {
    return text
  }
  return `${text.slice(0, QUOTE_MAX_CHARACTERS - 1).trimEnd()}…`
}

function drawnKind(value: unknown) {
  return DRAWN_MARK_KINDS.find((kind) => kind === value) ?? null
}

// How a capture a person uploaded reads, by what it holds.
function addedCaptureSentence(data: Record<string, unknown>): HistorySentence {
  const hasRecording = data.recording === true
  const hasTranscript = data.transcript === true
  if (hasRecording && hasTranscript) {
    return 'addedRecordingAndTranscript'
  }
  if (hasRecording) {
    return 'addedRecording'
  }
  if (hasTranscript) {
    return 'addedTranscript'
  }
  console.debug('describeHistory read an added capture that says it holds nothing', { data })
  return 'changed'
}

// What each entry of meeting `meetingId`'s history says, newest first as it arrives. Entries come
// from every version of the API, so their data is read defensively and an unknown kind reads as a
// change.
export function describeHistory(entries: MeetingHistoryEntry[], meetingId: string): HistoryRow[] {
  const undone = new Set<string>()
  for (const entry of entries) {
    if (entry.undid) {
      undone.add(entry.undid)
    }
  }

  return entries.map((entry) => {
    const row: HistoryRow = {
      entry,
      sentence: 'changed',
      before: null,
      after: null,
      markKind: null,
      personId: null,
      canUndo: UNDOABLE_KINDS.has(entry.kind)
        && !undone.has(entry.id)
        && entry.data.correction !== true,
    }
    switch (entry.kind) {
      case 'built':
        row.sentence = builtSentences.find(([ builtFrom ]) => builtFrom === entry.data.builtFrom)?.[1]
          ?? 'changed'
        return row
      case 'settled':
        // Its correction settled the transcript, flagging what it was unsure of.
        row.sentence = 'settled'
        return row
      case 'edited':
        // An undo of an edit is an edit back: its before is the edit's after.
        row.sentence = entry.undid
          ? 'undidEdit'
          : 'edited'
        row.before = quoted(entry.data.beforeText)
        row.after = quoted(entry.data.afterText)
        return row
      case 'marked':
      case 'unmarked':
        row.sentence = entry.kind
        row.markKind = drawnKind(entry.data.kind)
        return row
      case 'hold_changed':
        row.sentence = entry.data.after === true
          ? 'held'
          : 'released'
        return row
      case 'correction_approved':
        row.sentence = 'approved'
        return row
      case 'offset_changed':
        // An undo of a move is a move back to where the capture was.
        row.sentence = entry.undid
          ? 'undidOffset'
          : 'offsetChanged'
        return row
      case 'merged':
      case 'split': {
        const side = sideOf(entry, meetingId)
        if (!side) {
          return row
        }
        row.sentence = movedSentences[entry.kind][side]
        row.canUndo = side === 'kept' && !undone.has(entry.id)
        return row
      }
      case 'capture_added':
        row.sentence = addedCaptureSentence(entry.data)
        return row
      case 'thread_resolved':
        row.sentence = 'threadResolved'
        return row
      case 'thread_reopened':
        row.sentence = 'threadReopened'
        return row
      case 'visibility_changed':
        row.sentence = visibilitySentence(entry.data)
        row.after = quoted(entry.data.why)
        return row
      case 'shared':
      case 'unshared':
        row.sentence = entry.kind
        row.personId = typeof entry.data.personId === 'string'
          ? entry.data.personId
          : null
        return row
      default:
        console.debug('describeHistory read a change it does not know', { kind: entry.kind })
        return row
    }
  })
}

// The change that stood an edit's mark: the newest edit, or undo of one, that added it.
export function entryOfEditMark(entries: MeetingHistoryEntry[], markId: string) {
  return entries.find((entry) => entry.kind === 'edited'
    && Array.isArray(entry.data.marksAdded)
    && entry.data.marksAdded.includes(markId)) ?? null
}
