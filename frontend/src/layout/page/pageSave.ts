// Copyright © 2026 Jalapeno Labs

// Core
import { createContext, useContext, useEffect } from 'react'

// The one save a page has, as the form that owns it describes it. `Page` draws its Save
// button from this in the header, on the floating card, and for Ctrl+S.
export type PageSave = {
  // The `id` of the `<form>` Save submits. Submitting the form, rather than calling a
  // function, keeps one path for Save, Enter, and Ctrl+S, validation included.
  formId: string
  // Unsaved changes: Save is enabled, the floating card may show, and Esc asks first.
  isDirty: boolean
  isPending: boolean
  // Why Save cannot be pressed, shown in its tooltip. Save stays disabled while set.
  blockedReason?: string | null
  // Defaults to "Save". A /new page names what it does, such as "Add LLM".
  label?: string
}

type PageSaveRegistry = (save: PageSave | null) => void

export const PageSaveContext = createContext<PageSaveRegistry | null>(null)

// Gives the page around this form its Save. Call it from the component that owns the form,
// however deep, and put `save.formId` on that form's `id`; the form keeps its own submit
// handler. A page has one save, so one form per page calls this.
export function usePageSave(save: PageSave) {
  const register = useContext(PageSaveContext)
  if (!register) {
    throw new Error('usePageSave must be called inside a Page')
  }

  const { formId, isDirty, isPending, blockedReason, label } = save
  useEffect(() => {
    register({ formId, isDirty, isPending, blockedReason, label })
  }, [ register, formId, isDirty, isPending, blockedReason, label ])

  // Only an unmount takes the Save away, so the header button never flickers between
  // changes of the form's state.
  useEffect(() => () => register(null), [ register ])
}

// Whether the Save button can be pressed.
export function canPressSave(save: PageSave) {
  return save.isDirty && !save.isPending && !save.blockedReason
}

// Presses a page's Save, as a click on it would.
export function submitPageForm(save: PageSave) {
  const form = document.getElementById(save.formId)
  if (!(form instanceof HTMLFormElement)) {
    console.debug('submitPageForm found no form to submit', { formId: save.formId })
    return
  }
  form.requestSubmit()
}
