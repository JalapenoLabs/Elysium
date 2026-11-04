// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  LearningCategory,
  LearningContent,
  LearningHistoryEntry,
  LearningHistoryKind,
} from '../../../api/routes/learningRoutes'

// Misc
import { LEARNING_HISTORY_KINDS } from '../../../api/routes/learningRoutes'

// The bounds the API holds a learning to (api/src/routes/v1/learnings/save_learning.rs).
export const TITLE_MAX_CHARACTERS = 300
export const TEXT_MAX_CHARACTERS = 20_000

export const categoryLabelKeys = {
  decision: 'categories.decision',
  convention: 'categories.convention',
  procedure: 'categories.procedure',
  pitfall: 'categories.pitfall',
  reference: 'categories.reference',
  environment: 'categories.environment',
  people: 'categories.people',
  project: 'categories.project',
  incident: 'categories.incident',
} as const satisfies Record<LearningCategory, ParseKeys<'learnings'>>

// Mirrors the doc comments on `LearningCategory`, which agents read over MCP.
export const categoryDescriptionKeys = {
  decision: 'categoryDescriptions.decision',
  convention: 'categoryDescriptions.convention',
  procedure: 'categoryDescriptions.procedure',
  pitfall: 'categoryDescriptions.pitfall',
  reference: 'categoryDescriptions.reference',
  environment: 'categoryDescriptions.environment',
  people: 'categoryDescriptions.people',
  project: 'categoryDescriptions.project',
  incident: 'categoryDescriptions.incident',
} as const satisfies Record<LearningCategory, ParseKeys<'learnings'>>

// The fields a revision changes, in the order a learning reads.
export const LEARNING_FIELDS = [ 'title', 'body', 'category', 'evidence' ] as const satisfies (keyof LearningContent)[]
type LearningField = typeof LEARNING_FIELDS[number]

export const fieldLabelKeys = {
  title: 'history.fields.title',
  body: 'history.fields.body',
  category: 'history.fields.category',
  evidence: 'history.fields.evidence',
} as const satisfies Record<LearningField, ParseKeys<'learnings'>>

export const historySentenceKeys = {
  created: 'history.entries.created',
  revised: 'history.entries.revised',
  deleted: 'history.entries.deleted',
  restored: 'history.entries.restored',
} as const satisfies Record<LearningHistoryKind, ParseKeys<'learnings'>>

export const feedSentenceKeys = {
  created: 'feed.entries.created',
  revised: 'feed.entries.revised',
  deleted: 'feed.entries.deleted',
  restored: 'feed.entries.restored',
} as const satisfies Record<LearningHistoryKind, ParseKeys<'learnings'>>

// What an entry tells a reader at a glance: its kind (null for one a later API wrote, which
// still reads as a change), the fields a revision changed in reading order, and the earlier
// entry an undo reversed. Malformed data reads as no fields and no undo.
export function describeHistoryEntry(entry: LearningHistoryEntry) {
  const changes = entry.data.changes
  const changedFields: LearningField[] = []
  if (changes && typeof changes === 'object') {
    for (const field of LEARNING_FIELDS) {
      if (field in changes) {
        changedFields.push(field)
      }
    }
  }
  const undid = typeof entry.data.undid === 'string'
    ? entry.data.undid
    : null

  return {
    kind: LEARNING_HISTORY_KINDS.find((known) => known === entry.kind) ?? null,
    changedFields,
    undid,
  } as const
}

