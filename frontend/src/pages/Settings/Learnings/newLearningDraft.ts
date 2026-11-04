// Copyright © 2026 Jalapeno Labs

import type { LearningFormInput } from './learningFormSchema'

// Misc
import { NEW_LEARNING_STORAGE_KEY } from '../../../constants'

// What a new learning's form held, kept for this tab until it is saved, so a reload or a crash
// mid-write gives the words back. Storage can be unavailable or full; then nothing is kept and
// the form still works.

export function readNewLearningDraft(): LearningFormInput | null {
  try {
    const stored = window.sessionStorage.getItem(NEW_LEARNING_STORAGE_KEY)
    if (!stored) {
      return null
    }
    const parsed: unknown = JSON.parse(stored)
    if (!parsed || typeof parsed !== 'object') {
      console.debug('A new learning draft was not an object, so it was dropped')
      return null
    }
    const draft: Record<string, unknown> = { ...parsed }
    // Kept as it was typed: whether the category still exists is the form's to check, against
    // the categories it offers.
    return {
      title: String(draft.title ?? ''),
      body: String(draft.body ?? ''),
      category: String(draft.category ?? ''),
      evidence: String(draft.evidence ?? ''),
    }
  }
  catch (error) {
    console.debug('A new learning draft could not be read', { error })
    return null
  }
}

export function saveNewLearningDraft(values: Partial<LearningFormInput>) {
  // A blank form is never written over a draft: the form starts blank before the draft is put
  // back into it, and a blank write then would lose the draft.
  const hasText = [ values.title, values.body, values.evidence ].some((text) => text?.trim())
  if (!hasText) {
    return
  }
  try {
    window.sessionStorage.setItem(NEW_LEARNING_STORAGE_KEY, JSON.stringify(values))
  }
  catch (error) {
    console.debug('A new learning draft could not be kept for this tab', { error })
  }
}

export function forgetNewLearningDraft() {
  try {
    window.sessionStorage.removeItem(NEW_LEARNING_STORAGE_KEY)
  }
  catch (error) {
    console.debug('A new learning draft could not be forgotten', { error })
  }
}
