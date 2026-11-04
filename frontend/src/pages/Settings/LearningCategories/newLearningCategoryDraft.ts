// Copyright © 2026 Jalapeno Labs

import type { LearningCategoryFormInput } from './learningCategoryFormSchema'

// Misc
import { NEW_LEARNING_CATEGORY_STORAGE_KEY } from '../../../constants'

// What a new category's form held, kept for this tab until it is saved, as a new learning's is
// (`newLearningDraft.ts`). `isKeyTyped` remembers whether the key was typed by hand, so it goes
// back to following the name only if it was following it before.
export type NewLearningCategoryDraft = LearningCategoryFormInput & {
  isKeyTyped: boolean
}

export function readNewLearningCategoryDraft(): NewLearningCategoryDraft | null {
  try {
    const stored = window.sessionStorage.getItem(NEW_LEARNING_CATEGORY_STORAGE_KEY)
    if (!stored) {
      return null
    }
    const parsed: unknown = JSON.parse(stored)
    if (!parsed || typeof parsed !== 'object') {
      console.debug('A new learning category draft was not an object, so it was dropped')
      return null
    }
    const draft: Record<string, unknown> = { ...parsed }
    return {
      label: String(draft.label ?? ''),
      key: String(draft.key ?? ''),
      description: String(draft.description ?? ''),
      isKeyTyped: draft.isKeyTyped === true,
    }
  }
  catch (error) {
    console.debug('A new learning category draft could not be read', { error })
    return null
  }
}

export function saveNewLearningCategoryDraft(draft: Partial<NewLearningCategoryDraft>) {
  // A blank form is never written over a draft: the form starts blank before the draft is put
  // back into it, and a blank write then would lose the draft.
  const hasText = [ draft.label, draft.key, draft.description ].some((text) => text?.trim())
  if (!hasText) {
    return
  }
  try {
    window.sessionStorage.setItem(NEW_LEARNING_CATEGORY_STORAGE_KEY, JSON.stringify(draft))
  }
  catch (error) {
    console.debug('A new learning category draft could not be kept for this tab', { error })
  }
}

export function forgetNewLearningCategoryDraft() {
  try {
    window.sessionStorage.removeItem(NEW_LEARNING_CATEGORY_STORAGE_KEY)
  }
  catch (error) {
    console.debug('A new learning category draft could not be forgotten', { error })
  }
}
