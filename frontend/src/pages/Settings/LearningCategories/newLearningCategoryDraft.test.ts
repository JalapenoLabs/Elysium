// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it } from 'vitest'

// Misc
import { NEW_LEARNING_CATEGORY_STORAGE_KEY } from '../../../constants'
import {
  forgetNewLearningCategoryDraft,
  readNewLearningCategoryDraft,
  saveNewLearningCategoryDraft,
} from './newLearningCategoryDraft'

const draft = {
  label: 'EDA tools',
  key: 'eda',
  description: 'Simulators, synthesis, and their licenses.',
  isKeyTyped: true,
} as const

describe('newLearningCategoryDraft', () => {
  beforeEach(() => {
    window.sessionStorage.clear()
  })

  it('gives back what the form held, and whether the key was typed, until it is forgotten', () => {
    saveNewLearningCategoryDraft(draft)
    expect(readNewLearningCategoryDraft()).toEqual(draft)

    forgetNewLearningCategoryDraft()
    expect(readNewLearningCategoryDraft()).toBeNull()
  })

  it('never writes a blank form over a draft', () => {
    saveNewLearningCategoryDraft(draft)
    saveNewLearningCategoryDraft({ label: ' ', key: '', description: '\n', isKeyTyped: false })

    expect(readNewLearningCategoryDraft()).toEqual(draft)
  })

  it('drops a stored draft it cannot read rather than filling the form with it', () => {
    window.sessionStorage.setItem(NEW_LEARNING_CATEGORY_STORAGE_KEY, '{ not json')
    expect(readNewLearningCategoryDraft()).toBeNull()
  })
})
