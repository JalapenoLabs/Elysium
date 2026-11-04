// Copyright © 2026 Jalapeno Labs

import type { FieldValues, Path, PathValue, UseFormReturn } from 'react-hook-form'

// Core
import { useCallback, useEffect, useRef } from 'react'

// Misc
import { forgetSessionDraft, keepSessionDraft, readSessionDraft } from './sessionDrafts'

// A field a draft keeps, named as the form names it, such as `title`.
type DraftField<Values extends FieldValues> = Path<Values>

type Options<Values extends FieldValues> = {
  // What the draft is kept under, such as `action-item`; one per form, or per record a form
  // is about (`coding-session.item-<id>`).
  name: string
  // The text fields the draft keeps, what is typed. Never a secret, and never a field the page
  // presets from its address, which a stale draft would override. A function decides from the
  // values as they are, for a field that is secret only sometimes.
  fields: readonly DraftField<Values>[] | ((values: Values) => readonly DraftField<Values>[])
  // Choices kept beside the text, such as a priority or a switch. They come back with the text
  // but are never worth a draft on their own.
  choices?: readonly DraftField<Values>[]
  // False for the same form editing a record that exists, which keeps nothing.
  isEnabled?: boolean
  // Called with what was put back, for state the form derives beside its fields, such as
  // whether a key was typed by hand or still follows the name.
  onRestore?: (restored: Partial<Values>) => void
}

// Whether a stored value can go back into a field holding `current`: the same kind of value,
// and for a list, a list of text such as picked ids. Storage is outside the app, so nothing in
// it is trusted to fit.
function fitsField(current: unknown, stored: unknown) {
  if (Array.isArray(current)) {
    return Array.isArray(stored) && stored.every((entry) => typeof entry === 'string')
  }
  return current !== null && typeof current !== 'object' && typeof stored === typeof current
}

// Keeps a new form's input as a draft for the tab (`~/.claude/docs/ux.md`, Form inputs): the
// listed fields go back into the blank form as changes when it opens, so Save is ready and
// leaving asks first, and every change after that is kept. Returns `forget`, for the form to
// call once it creates what it describes; register it as the page's `onDiscard` too, so
// Discard work in the leave prompt ends the draft rather than bringing it back next visit.
export function useFormDraft<Values extends FieldValues, Submitted>(
  form: UseFormReturn<Values, unknown, Submitted>,
  options: Options<Values>,
) {
  const {
    name,
    isEnabled = true,
  } = options
  // The latest list and callback, read when the draft is kept or put back, so neither needs a
  // new effect.
  const fieldsRef = useRef(options.fields)
  const choicesRef = useRef(options.choices ?? [])
  const onRestoreRef = useRef(options.onRestore)
  useEffect(() => {
    fieldsRef.current = options.fields
    choicesRef.current = options.choices ?? []
    onRestoreRef.current = options.onRestore
  })

  useEffect(() => {
    if (!isEnabled) {
      return undefined
    }

    function listTextFields() {
      const fields = fieldsRef.current
      return typeof fields === 'function'
        ? fields(form.getValues())
        : fields
    }

    const stored = readSessionDraft(name)
    if (stored !== null && (typeof stored !== 'object' || Array.isArray(stored))) {
      console.debug('A form draft was not an object, so it was left out', { name })
    }
    else if (stored) {
      const restored: Partial<Values> = {}
      for (const field of [ ...listTextFields(), ...choicesRef.current ]) {
        if (!(field in stored)) {
          continue
        }
        const value: unknown = Reflect.get(stored, field)
        if (!fitsField(form.getValues(field), value)) {
          console.debug('A form draft held a value that does not fit its field, so it was left out', { name, field })
          continue
        }
        // Checked by `fitsField` against the field's own value just above.
        form.setValue(field, value as PathValue<Values, typeof field>, { shouldDirty: true })
        Reflect.set(restored, field, value)
      }
      onRestoreRef.current?.(restored)
    }

    // A subscription rather than an effect on the watched values, so it hears only changes:
    // the form's defaults, before the draft is put back, are never written over it.
    return form.subscribe({
      formState: { values: true },
      callback: () => {
        const draft: Record<string, unknown> = {}
        const typed: unknown[] = []
        // Listed from the values as they are now, so a value that just turned secret is
        // left out of the very write its change makes.
        for (const field of listTextFields()) {
          draft[field] = form.getValues(field)
          typed.push(draft[field])
        }
        for (const field of choicesRef.current) {
          draft[field] = form.getValues(field)
        }
        keepSessionDraft(name, draft, typed)
      },
    })
  }, [ form, name, isEnabled ])

  return useCallback(() => forgetSessionDraft(name), [ name ])
}
