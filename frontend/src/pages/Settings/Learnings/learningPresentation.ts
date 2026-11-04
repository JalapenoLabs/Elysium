// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'
import type {
  BuiltInLearningCategory,
  LearningContent,
  LearningHistoryEntry,
  LearningHistoryKind,
} from '../../../api/routes/learningRoutes'

// Misc
import { BUILT_IN_LEARNING_CATEGORIES, LEARNING_HISTORY_KINDS } from '../../../api/routes/learningRoutes'

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
} as const satisfies Record<BuiltInLearningCategory, ParseKeys<'learnings'>>

// Mirrors the built-in descriptions the API seeds, which agents read over MCP.
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
} as const satisfies Record<BuiltInLearningCategory, ParseKeys<'learnings'>>

// A category as people read it: a built-in one in this app's words, from its translations, and
// the workspace's own as its admins wrote it.
export type PresentedLearningCategory = {
  key: string
  label: string
  description: string
  builtIn: boolean
}

export function presentLearningCategory(
  category: LearningCategory,
  t: TFunction<'learnings'>,
): PresentedLearningCategory {
  // A built-in key this app has no words for, such as one a later API seeds, reads as the API
  // names it.
  const builtIn = category.builtIn
    ? BUILT_IN_LEARNING_CATEGORIES.find((known) => known === category.key)
    : undefined
  if (!builtIn) {
    return {
      key: category.key,
      label: category.label,
      description: category.description,
      builtIn: category.builtIn,
    }
  }
  return {
    key: category.key,
    label: t(categoryLabelKeys[builtIn]),
    description: t(categoryDescriptionKeys[builtIn]),
    builtIn: true,
  }
}

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

