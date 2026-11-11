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
  edited: 'review.history.sentences.edited',
  undidEdit: 'review.history.sentences.undidEdit',
  marked: 'review.history.sentences.marked',
  unmarked: 'review.history.sentences.unmarked',
  held: 'review.history.sentences.held',
  released: 'review.history.sentences.released',
  approved: 'review.history.sentences.approved',
  offsetChanged: 'review.history.sentences.offsetChanged',
  merged: 'review.history.sentences.merged',
  split: 'review.history.sentences.split',
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
  // Whether Undo applies: a change that can be undone and was not undone already.
  canUndo: boolean
}

// Long enough to recognize a phrase, short enough for a narrow lane.
const QUOTE_MAX_CHARACTERS = 60

// A merge and a split are in both meetings' histories, and undone from the one the recordings went
// into, by splitting them back out, so the list offers no Undo for either.
const UNDOABLE_KINDS = new Set([ 'edited', 'marked', 'unmarked', 'hold_changed', 'offset_changed' ])

// What a transcript was built from (`BuiltFrom` in api/src/models/meeting_word.rs), and how its
// build reads.
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

// What each entry of a meeting's history says, newest first as it arrives. Entries come from
// every version of the API, so their data is read defensively and an unknown kind reads as a
// change.
export function describeHistory(entries: MeetingHistoryEntry[]): HistoryRow[] {
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
      canUndo: UNDOABLE_KINDS.has(entry.kind) && !undone.has(entry.id),
    }
    switch (entry.kind) {
      case 'built':
        row.sentence = builtSentences.find(([ builtFrom ]) => builtFrom === entry.data.builtFrom)?.[1]
          ?? 'changed'
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
        row.sentence = 'offsetChanged'
        return row
      case 'merged':
      case 'split':
        row.sentence = entry.kind
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
