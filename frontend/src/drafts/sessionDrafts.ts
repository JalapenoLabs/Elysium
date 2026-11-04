// Copyright © 2026 Jalapeno Labs

// Misc
import { DRAFT_STORAGE_PREFIX } from '../constants'

// What a person has typed and not yet saved or sent, kept in session storage for the tab, so a
// reload or a crash mid-write gives it back (`~/.claude/docs/ux.md`, Form inputs). A draft is
// named by what it belongs to, such as `action-item` for New item or `reply.<recordId>` for a
// record's reply box. Storage can be unavailable (an old browser, a private window) or full;
// then nothing is kept and every form still works. Secrets are never handed to it: the forms
// list the fields they keep, and a secret field is never among them.

export type SessionDraft = string | Record<string, unknown>

// The draft kept under `name`, as it was kept, or null when there is none or it cannot be read.
// What it holds is unchecked: storage is outside the app, so callers check each value's shape
// before using it.
export function readSessionDraft(name: string): unknown {
  try {
    const stored = window.sessionStorage.getItem(DRAFT_STORAGE_PREFIX + name)
    if (stored === null) {
      return null
    }
    return JSON.parse(stored)
  }
  catch (error) {
    console.debug('A draft could not be read, so it was left out', { error, name })
    return null
  }
}

// Keeps `draft` under `name`, replacing what was kept. `typed` is what the person typed into
// it, such as its text boxes, apart from choices like a priority. While all of that is blank the
// draft is never written: blanking a field, or a form starting blank before its draft is put
// back, must not wipe out what was typed, and a choice alone is not worth giving back. A draft
// ends only when `forgetSessionDraft` is called, once it is saved, sent, or discarded on purpose.
export function keepSessionDraft(name: string, draft: SessionDraft, typed: readonly unknown[]) {
  const holdsText = typed.some((value) => typeof value === 'string' && value.trim())
  if (!holdsText) {
    return
  }
  try {
    window.sessionStorage.setItem(DRAFT_STORAGE_PREFIX + name, JSON.stringify(draft))
  }
  catch (error) {
    console.debug('A draft could not be kept for this tab', { error, name })
  }
}

export function forgetSessionDraft(name: string) {
  try {
    window.sessionStorage.removeItem(DRAFT_STORAGE_PREFIX + name)
  }
  catch (error) {
    console.debug('A draft could not be forgotten', { error, name })
  }
}
