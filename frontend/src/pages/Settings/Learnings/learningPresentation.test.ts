// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { LEARNING_CATEGORIES } from '../../../api/routes/learningRoutes'
import { makeLearningHistoryEntry } from '../../../testFixtures'
import { createLearningFormSchema } from './learningFormSchema'
import {
  categoryDescriptionKeys,
  categoryLabelKeys,
  describeHistoryEntry,
  TEXT_MAX_CHARACTERS,
} from './learningPresentation'

describe('describeHistoryEntry', () => {
  it('names the fields a revision changed, in the order a learning reads', () => {
    const entry = makeLearningHistoryEntry({
      id: 'revision',
      kind: 'revised',
      data: {
        changes: {
          evidence: { from: 'a', to: 'b' },
          title: { from: 'c', to: 'd' },
        },
      },
    })

    expect(describeHistoryEntry(entry)).toEqual({
      kind: 'revised',
      changedFields: [ 'title', 'evidence' ],
      undid: null,
    })
  })

  it('names the entry an undo reversed', () => {
    const entry = makeLearningHistoryEntry({ id: 'undo', kind: 'restored', data: { undid: 'deletion' }})

    expect(describeHistoryEntry(entry).undid).toBe('deletion')
  })

  it('still reads a kind a later API wrote, and malformed data, as some change', () => {
    const entry = makeLearningHistoryEntry({
      id: 'future',
      kind: 'merged',
      data: { changes: 'not an object', undid: 7 },
    })

    expect(describeHistoryEntry(entry)).toEqual({ kind: null, changedFields: [], undid: null })
  })
})

describe('categories', () => {
  it('label and describe every category the API files learnings under', () => {
    for (const category of LEARNING_CATEGORIES) {
      expect(i18next.exists(`learnings:${categoryLabelKeys[category]}`)).toBe(true)
      expect(i18next.exists(`learnings:${categoryDescriptionKeys[category]}`)).toBe(true)
    }
  })
})

describe('createLearningFormSchema', () => {
  const schema = createLearningFormSchema(i18next.getFixedT('en-US', 'learnings'))
  const valid = {
    title: '  Pin   every tool version ',
    category: 'convention',
    body: ' Never use latest. ',
    evidence: 'CI drifted from production.',
  }

  it('keeps the takeaway on one line and trims the text, as the API does', () => {
    expect(schema.parse(valid)).toEqual({
      title: 'Pin every tool version',
      category: 'convention',
      body: 'Never use latest.',
      evidence: 'CI drifted from production.',
    })
  })

  it('refuses a blank field, an oversized one, and an unknown category', () => {
    expect(schema.safeParse({ ...valid, title: ' \n ' }).success).toBe(false)
    expect(schema.safeParse({ ...valid, evidence: '   ' }).success).toBe(false)
    expect(schema.safeParse({ ...valid, body: 'b'.repeat(TEXT_MAX_CHARACTERS + 1) }).success).toBe(false)
    expect(schema.safeParse({ ...valid, category: 'gotcha' }).success).toBe(false)
  })
})
