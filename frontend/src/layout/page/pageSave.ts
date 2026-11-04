// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { FieldValues, UseFormReturn } from 'react-hook-form'

import type { RefCallback } from 'react'

// Core
import { createContext, useCallback, useContext, useEffect, useRef } from 'react'

// Misc
import { TAB_PARAM } from '../../urls'

// The one save a page has, as the form that owns it describes it. `Page` draws its Save
// button from this in the header and on the floating card, presses it for Ctrl+S and
// Enter, and runs it from the leave prompt's Save and continue.
export type PageSave = {
  // Saves the form, resolving true once saved and false when not: invalid values, which the
  // fields show, or a refusal, which the form reports itself with a toast. A function rather
  // than a form submit, so the leave prompt can tell whether it may go on.
  save: () => Promise<boolean>
  // Unsaved changes: Save is enabled, the floating card may show, and leaving asks first.
  isDirty: boolean
  // Why Save cannot be pressed, shown in its tooltip. Save stays disabled while set.
  blockedReason?: string | null
  // Defaults to "Save". A /new page names what it does, such as "Add LLM".
  label?: string
  // Save can be pressed with nothing changed, for a /new page whose create needs nothing typed,
  // such as starting a browser. Leaving still asks only while the form is dirty.
  canSaveUnchanged?: boolean
  // Called when the person leaves choosing Discard work, such as to end the form's draft
  // (`useFormDraft`), so discarded words never come back on the next visit.
  onDiscard?: () => void
}

export type PageSaveRegistry = {
  register: (save: PageSave | null) => void
  // Runs the registered save, as pressing Save does. Resolves whether it saved.
  runSave: () => Promise<boolean>
}

export const PageSaveContext = createContext<PageSaveRegistry | null>(null)

// The page's Save as it stands, for the controls that mirror it below a form: a /new page's
// Back and Create (`PageCreateFooter`). Kept apart from the registry, which forms hold and
// which never changes, so a save starting re-renders only what shows it.
export type PageSaveState = {
  save: PageSave | null
  isSaving: boolean
  runSave: () => Promise<boolean>
  // Where Back goes: the page's `escapeTo`, or its last parent.
  backTo: string | undefined
  // Given the footer, so the page knows when its Create is in view.
  createFooterRef: RefCallback<HTMLDivElement>
}

export const PageSaveStateContext = createContext<PageSaveState | null>(null)

// Gives the page around this form its Save, and returns the form's `onSubmit`, so Enter
// saves through the page like every other way of saving. Call it from the component that
// owns the form, however deep. A page has one save, so one form per page calls this.
export function usePageSave(pageSave: PageSave) {
  const registry = useContext(PageSaveContext)
  if (!registry) {
    throw new Error('usePageSave must be called inside a Page')
  }

  // The form makes new save and discard functions every render. The page holds ones that call
  // the latest, so registering never churns the page's state between keystrokes.
  const saveRef = useRef(pageSave.save)
  const onDiscardRef = useRef(pageSave.onDiscard)
  useEffect(() => {
    saveRef.current = pageSave.save
    onDiscardRef.current = pageSave.onDiscard
  })

  const { register, runSave } = registry
  const { isDirty, blockedReason, label, canSaveUnchanged } = pageSave
  useEffect(() => {
    register({
      save: () => saveRef.current(),
      onDiscard: () => onDiscardRef.current?.(),
      isDirty,
      blockedReason,
      label,
      canSaveUnchanged,
    })
  }, [ register, isDirty, blockedReason, label, canSaveUnchanged ])

  // Only an unmount takes the Save away, so the header button never flickers between
  // changes of the form's state.
  useEffect(() => () => register(null), [ register ])

  return useCallback((event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    void runSave()
  }, [ runSave ])
}

// A react-hook-form form's `save` for `usePageSave`: its validation first, then `onValid`,
// which resolves true once the values are saved. Invalid values resolve false, with the
// fields saying why.
export function saveForm<Input extends FieldValues, Output>(
  form: UseFormReturn<Input, unknown, Output>,
  onValid: (values: Output) => Promise<boolean>,
) {
  return async () => {
    let isSaved = false
    await form.handleSubmit(
      async (values) => {
        isSaved = await onValid(values)
      },
      (errors) => console.debug('saveForm kept the form: its values are invalid', { fields: Object.keys(errors) }),
    )()
    return isSaved
  }
}

// Navigation state for leaving a page whose record is gone, such as straight after deleting
// it: nothing is left to save, so the leave prompt does not ask.
// `navigate(UrlTree.projects, { state: LEAVE_WITHOUT_ASKING })`
export const LEAVE_WITHOUT_ASKING = { leavesWithoutAsking: true } as const

export function leavesWithoutAsking(state: unknown) {
  return typeof state === 'object'
    && state !== null
    && 'leavesWithoutAsking' in state
    && state.leavesWithoutAsking === true
}

// Whether going from one address of a page to another, both on the same path, can swap what
// the page edits, so the leave prompt must ask first. A query such as Jira's `?project=` picks
// the record a form edits. The open tab (`?tab=`) never does: it shows another part of the
// same page, its form still mounted, so nothing typed is lost by switching.
export function swapsWhatPageEdits(currentSearch: string, nextSearch: string) {
  const current = new URLSearchParams(currentSearch)
  const next = new URLSearchParams(nextSearch)
  current.delete(TAB_PARAM)
  next.delete(TAB_PARAM)
  current.sort()
  next.sort()
  return current.toString() !== next.toString()
}

// Whether the Save button can be pressed.
export function canPressSave(save: PageSave, isSaving: boolean) {
  const hasSomethingToSave = save.isDirty || Boolean(save.canSaveUnchanged)
  return hasSomethingToSave && !isSaving && !save.blockedReason
}
