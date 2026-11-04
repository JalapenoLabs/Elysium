// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  learningCategoriesLoaded,
  learningCategoryRemoved,
  learningCategoryUpserted,
  selectLearningCategories,
} from './learningCategoriesSlice'

// Misc
import { makeLearningCategory } from '../testFixtures'

describe('learningCategoriesSlice', () => {
  it('offers the built-in categories first, then the workspace\'s own, each in its order', () => {
    const store = createAppStore()
    store.dispatch(learningCategoriesLoaded([
      makeLearningCategory({ key: 'verification', position: 1 }),
      makeLearningCategory({ key: 'pitfall', position: 3, builtIn: true }),
      makeLearningCategory({ key: 'rtl', position: 0 }),
      makeLearningCategory({ key: 'decision', position: 0, builtIn: true }),
    ]))

    const keys = selectLearningCategories(store.getState()).map((category) => category.key)
    expect(keys).toEqual([ 'decision', 'pitfall', 'rtl', 'verification' ])
  })

  it('moves an edited category to its new place, and forgets a removed one', () => {
    const store = createAppStore()
    store.dispatch(learningCategoriesLoaded([
      makeLearningCategory({ key: 'rtl', position: 0 }),
      makeLearningCategory({ key: 'soc', position: 1 }),
    ]))
    store.dispatch(learningCategoryUpserted(makeLearningCategory({ key: 'soc', position: 0, label: 'SoC' })))
    store.dispatch(learningCategoryUpserted(makeLearningCategory({ key: 'rtl', position: 1 })))

    const categories = selectLearningCategories(store.getState())
    expect(categories.map((category) => category.key)).toEqual([ 'soc', 'rtl' ])
    expect(categories[0].label).toBe('SoC')

    store.dispatch(learningCategoryRemoved('rtl'))
    expect(selectLearningCategories(store.getState()).map((category) => category.key)).toEqual([ 'soc' ])
  })
})
