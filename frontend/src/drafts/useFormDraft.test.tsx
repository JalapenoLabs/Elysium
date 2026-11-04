// Copyright © 2026 Jalapeno Labs

// Core
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, screen } from '@testing-library/react'
import { useForm, useWatch } from 'react-hook-form'

// Misc
import { keepSessionDraft, readSessionDraft } from './sessionDrafts'
import { useFormDraft } from './useFormDraft'

type Values = {
  key: string
  value: string
  isSecret: boolean
  projectIds: string[]
}

type Props = {
  isEnabled?: boolean
  onRestore?: (restored: Partial<Values>) => void
  // Hands the test the form and the draft's forget, to act on them as the form would.
  onReady: (form: ReturnType<typeof useForm<Values>>, forget: () => void) => void
}

// A form like an environment variable's: a key, a value kept only while it is not secret,
// and a preset list the draft leaves alone.
function DraftedForm(props: Props) {
  const form = useForm<Values>({
    defaultValues: { key: '', value: '', isSecret: false, projectIds: [ 'preset' ]},
  })
  const forget = useFormDraft(form, {
    name: 'variable',
    fields: (current) => current.isSecret
      ? [ 'key' ]
      : [ 'key', 'value' ],
    choices: [ 'isSecret' ],
    isEnabled: props.isEnabled,
    onRestore: props.onRestore,
  })
  const values = useWatch({ control: form.control })
  props.onReady(form, forget)

  return <p>{`${values.key}|${values.value}|${String(values.isSecret)}|${form.formState.isDirty}`}</p>
}

function renderForm(props: Partial<Props> = {}) {
  let form: ReturnType<typeof useForm<Values>> | undefined
  let forget: (() => void) | undefined
  const result = render(<DraftedForm
    {...props}
    onReady={(readyForm, readyForget) => {
      form = readyForm
      forget = readyForget
    }}
  />)
  if (!form || !forget) {
    throw new Error('the form did not render')
  }
  return { ...result, form, forget }
}

describe('useFormDraft', () => {
  beforeEach(() => {
    window.sessionStorage.clear()
  })

  it('puts a kept draft back into the blank form as changes, so the form is dirty', () => {
    keepSessionDraft('variable', { key: 'API_URL', value: 'https://example.com', isSecret: false }, [ 'API_URL' ])

    renderForm()

    expect(screen.getByText('API_URL|https://example.com|false|true')).toBeTruthy()
  })

  it('never writes a choice changed on its own, with nothing typed', () => {
    const { form } = renderForm()

    act(() => form.setValue('isSecret', true, { shouldDirty: true }))

    expect(readSessionDraft('variable')).toBeNull()
  })

  it('keeps every change to the fields it lists', () => {
    const { form } = renderForm()

    act(() => form.setValue('key', 'API_URL', { shouldDirty: true }))
    act(() => form.setValue('value', 'https://example.com', { shouldDirty: true }))

    expect(readSessionDraft('variable')).toEqual({ key: 'API_URL', value: 'https://example.com', isSecret: false })
  })

  it('leaves a field out while it is secret, from the very change that makes it so', () => {
    const { form } = renderForm()
    act(() => form.setValue('key', 'TOKEN', { shouldDirty: true }))

    act(() => form.setValue('isSecret', true, { shouldDirty: true }))
    act(() => form.setValue('value', 'hunter2', { shouldDirty: true }))

    expect(readSessionDraft('variable')).toEqual({ key: 'TOKEN', isSecret: true })
  })

  it('never puts back a field it does not list, such as one preset from the address', () => {
    keepSessionDraft('variable', { key: 'API_URL', projectIds: [ 'stale' ]}, [ 'API_URL' ])

    const { form } = renderForm()

    expect(form.getValues('projectIds')).toEqual([ 'preset' ])
  })

  it('leaves out a stored value that does not fit its field', () => {
    keepSessionDraft('variable', { key: 'API_URL', isSecret: 'yes', value: 42 }, [ 'API_URL' ])

    renderForm()

    expect(screen.getByText('API_URL||false|true')).toBeTruthy()
  })

  it('tells the form what it put back, for state the form derives beside its fields', () => {
    keepSessionDraft('variable', { key: 'API_URL', isSecret: false, value: '' }, [ 'API_URL' ])
    const onRestore = vi.fn()

    renderForm({ onRestore })

    expect(onRestore).toHaveBeenCalledWith({ key: 'API_URL', value: '', isSecret: false })
  })

  it('keeps nothing and puts nothing back for a form editing what exists', () => {
    keepSessionDraft('variable', { key: 'API_URL' }, [ 'API_URL' ])

    const { form } = renderForm({ isEnabled: false })
    act(() => form.setValue('key', 'OTHER', { shouldDirty: true }))

    expect(screen.getByText('OTHER||false|true')).toBeTruthy()
    expect(readSessionDraft('variable')).toEqual({ key: 'API_URL' })
  })

  it('ends the draft when the form forgets it', () => {
    const { form, forget } = renderForm()
    act(() => form.setValue('key', 'API_URL', { shouldDirty: true }))

    forget()

    expect(readSessionDraft('variable')).toBeNull()
  })
})
