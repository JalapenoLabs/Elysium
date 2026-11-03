// Copyright © 2026 Jalapeno Labs

// Core
import { createContext, useContext, useEffect } from 'react'

// The one save a settings page has, as the form that owns it describes it. `SettingsPage`
// draws its Save button from this in the header, on the floating card, and for Ctrl+S.
export type SettingsSave = {
  // The `id` of the `<form>` Save submits. Submitting the form, rather than calling a
  // function, keeps one path for Save, Enter, and Ctrl+S, validation included.
  formId: string
  // Unsaved changes: Save is enabled, the floating card may show, and Esc asks first.
  isDirty: boolean
  isPending: boolean
  // Why Save cannot be pressed, shown in its tooltip. Save stays disabled while set.
  blockedReason?: string | null
  // Defaults to "Save". An add page names what it does, such as "Add LLM".
  label?: string
}

type SettingsSaveRegistry = (save: SettingsSave | null) => void

export const SettingsSaveContext = createContext<SettingsSaveRegistry | null>(null)

// Gives the settings page around this form its Save. Call it from the component that owns
// the form, however deep, and put `save.formId` on that form's `id`; the form keeps its own
// submit handler and needs no button of its own. A page has one save, so one form per page
// calls this.
export function useSettingsSave(save: SettingsSave) {
  const register = useContext(SettingsSaveContext)
  if (!register) {
    throw new Error('useSettingsSave must be called inside a SettingsPage')
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
export function canPressSave(save: SettingsSave) {
  return save.isDirty && !save.isPending && !save.blockedReason
}

// Presses a settings page's Save, as a click on it would.
export function submitSettingsForm(save: SettingsSave) {
  const form = document.getElementById(save.formId)
  if (!(form instanceof HTMLFormElement)) {
    console.debug('submitSettingsForm found no form to submit', { formId: save.formId })
    return
  }
  form.requestSubmit()
}
