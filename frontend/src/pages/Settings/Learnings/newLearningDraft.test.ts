// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it } from 'vitest'

// Misc
import { NEW_LEARNING_STORAGE_KEY } from '../../../constants'
import { forgetNewLearningDraft, readNewLearningDraft, saveNewLearningDraft } from './newLearningDraft'

const draft = {
  title: 'Pin every tool version',
  category: 'convention',
  body: 'Never use latest.',
  evidence: '',
} as const

describe('newLearningDraft', () => {
  beforeEach(() => {
    window.sessionStorage.clear()
  })

  it('gives back what the form held until it is forgotten', () => {
    saveNewLearningDraft(draft)
    expect(readNewLearningDraft()).toEqual(draft)

    forgetNewLearningDraft()
    expect(readNewLearningDraft()).toBeNull()
  })

  it('never writes a blank form over a draft', () => {
    saveNewLearningDraft(draft)
    saveNewLearningDraft({ title: ' ', category: 'decision', body: '', evidence: '\n' })

    expect(readNewLearningDraft()).toEqual(draft)
  })

  it('drops a stored draft it cannot read rather than filling the form with it', () => {
    window.sessionStorage.setItem(NEW_LEARNING_STORAGE_KEY, '{ not json')
    expect(readNewLearningDraft()).toBeNull()

    window.sessionStorage.setItem(NEW_LEARNING_STORAGE_KEY, JSON.stringify({ ...draft, category: 'gotcha' }))
    expect(readNewLearningDraft()).toBeNull()
  })
})
