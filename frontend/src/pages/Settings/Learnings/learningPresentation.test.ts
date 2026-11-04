// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'
import { describe, expect, it } from 'vitest'

// Misc
import { BUILT_IN_LEARNING_CATEGORIES } from '../../../api/routes/learningRoutes'
import { makeLearningCategory, makeLearningHistoryEntry } from '../../../testFixtures'
import { createLearningFormSchema } from './learningFormSchema'
import {
  categoryDescriptionKeys,
  categoryLabelKeys,
  describeHistoryEntry,
  presentLearningCategory,
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
  it('label and describe every built-in category', () => {
    for (const category of BUILT_IN_LEARNING_CATEGORIES) {
      expect(i18next.exists(`learnings:${categoryLabelKeys[category]}`)).toBe(true)
      expect(i18next.exists(`learnings:${categoryDescriptionKeys[category]}`)).toBe(true)
    }
  })
})

describe('presentLearningCategory', () => {
  const t = i18next.getFixedT('en-US', 'learnings')

  it('names a built-in category in the app\'s own words', () => {
    const pitfall = makeLearningCategory({
      key: 'pitfall',
      label: 'Pitfall (as seeded)',
      description: 'As seeded.',
      builtIn: true,
    })

    expect(presentLearningCategory(pitfall, t)).toEqual({
      key: 'pitfall',
      label: 'Pitfall',
      description: 'Something that behaves other than expected, and the way around it',
      builtIn: true,
    })
  })

  it('names the workspace\'s own as its admins wrote it', () => {
    const custom = makeLearningCategory({ key: 'eda_tools', label: 'EDA tools', description: 'Simulators.' })

    expect(presentLearningCategory(custom, t)).toEqual({
      key: 'eda_tools',
      label: 'EDA tools',
      description: 'Simulators.',
      builtIn: false,
    })
  })

  it('names a built-in category it has no words for as the API does', () => {
    const later = makeLearningCategory({ key: 'hazard', label: 'Hazard', description: 'Later.', builtIn: true })

    expect(presentLearningCategory(later, t).label).toBe('Hazard')
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

  it('refuses a blank field, an oversized one, and no category', () => {
    expect(schema.safeParse({ ...valid, title: ' \n ' }).success).toBe(false)
    expect(schema.safeParse({ ...valid, evidence: '   ' }).success).toBe(false)
    expect(schema.safeParse({ ...valid, body: 'b'.repeat(TEXT_MAX_CHARACTERS + 1) }).success).toBe(false)
    expect(schema.safeParse({ ...valid, category: '' }).success).toBe(false)
  })

  it('takes one of the workspace\'s own categories, which only the API can check', () => {
    expect(schema.parse({ ...valid, category: 'eda_tools' }).category).toBe('eda_tools')
  })
})
