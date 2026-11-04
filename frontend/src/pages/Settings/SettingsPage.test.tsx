// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'
import type { Mock } from 'vitest'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { MemoryRouter, Route, Routes } from 'react-router'

// User interface
import { ConfirmGate } from '../../gates/ConfirmGate'
import { SettingsPage } from './SettingsPage'

// Misc
import { usePageSave } from '../../layout/page/pageSave'

const FORM_ID = 'example-settings-form'

type Observed = {
  callback: IntersectionObserverCallback
}

type Context = {
  observed: Observed[]
  onSubmit: Mock<() => void>
}

// Reports the header's Save as in or out of view, as scrolling would.
function setHeaderSaveVisible(context: Context, isVisible: boolean) {
  const observer = context.observed.at(-1)
  if (!observer) {
    throw new Error('the header Save is not observed')
  }
  act(() => {
    observer.callback([ { isIntersecting: isVisible } as IntersectionObserverEntry ], {} as IntersectionObserver)
  })
}

type FormProps = {
  onSubmit: () => void
}

function ExampleForm(props: FormProps) {
  const [ value, setValue ] = useState('')
  usePageSave({ formId: FORM_ID, isDirty: value !== '', isPending: false })

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    props.onSubmit()
  }

  return <form id={FORM_ID} onSubmit={submit}>
    <input aria-label='Value' value={value} onChange={(event) => setValue(event.target.value)} />
  </form>
}

function renderPage(context: Context) {
  return render(<MemoryRouter initialEntries={[ '/settings/llms/1/edit' ]}>
    <ConfirmGate>
      <Routes>
        <Route path='/settings' element={<p>Settings directory</p>} />
        <Route path='/settings/llms' element={<p>LLM list</p>} />
        <Route path='/settings/llms/1/edit' element={<SettingsPage
          title='Edit LLM'
          parents={[{ label: 'Manage LLMs', href: '/settings/llms' }]}
        >
          <ExampleForm onSubmit={context.onSubmit} />
        </SettingsPage>} />
      </Routes>
    </ConfirmGate>
  </MemoryRouter>)
}

describe('SettingsPage', () => {
  beforeEach<Context>((context) => {
    context.observed = []
    context.onSubmit = vi.fn<() => void>()
    vi.stubGlobal('IntersectionObserver', class {
      constructor(callback: IntersectionObserverCallback) {
        context.observed.push({ callback })
      }
      observe() {}
      disconnect() {}
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it<Context>('puts the one Save in the header, enabled once something changed', (context) => {
    renderPage(context)

    const save = screen.getByRole('button', { name: 'Save' })
    expect(save.closest('header')).not.toBeNull()
    expect(save.hasAttribute('disabled') || save.getAttribute('data-disabled') === 'true').toBe(true)

    fireEvent.change(screen.getByLabelText('Value'), { target: { value: 'changed' }})
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(context.onSubmit).toHaveBeenCalledTimes(1)
  })

  it<Context>('presses Save on Ctrl+S, and never opens the browser\'s save dialog', (context) => {
    renderPage(context)

    const untouched = fireEvent.keyDown(window, { key: 's', ctrlKey: true })
    expect(untouched).toBe(false)
    expect(context.onSubmit).not.toHaveBeenCalled()

    fireEvent.change(screen.getByLabelText('Value'), { target: { value: 'changed' }})
    fireEvent.keyDown(window, { key: 's', ctrlKey: true })
    expect(context.onSubmit).toHaveBeenCalledTimes(1)
  })

  it<Context>('goes back to the parent page on Esc', async (context) => {
    renderPage(context)

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(await screen.findByText('LLM list')).toBeTruthy()
  })

  it<Context>('asks before Esc leaves unsaved changes behind', async (context) => {
    renderPage(context)
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: 'changed' }})

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(await screen.findByRole('alertdialog')).toBeTruthy()
    expect(screen.queryByText('LLM list')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Leave' }))
    expect(await screen.findByText('LLM list')).toBeTruthy()
  })

  it<Context>('offers Save on a card while the header\'s is out of view, until dismissed', async (context) => {
    renderPage(context)
    fireEvent.change(screen.getByLabelText('Value'), { target: { value: 'changed' }})

    expect(screen.queryByRole('region', { name: 'Save changes' })).toBeNull()
    setHeaderSaveVisible(context, false)
    const card = screen.getByRole('region', { name: 'Save changes' })
    expect(card.textContent).toContain('You have unsaved changes')

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    await waitFor(() => expect(screen.queryByRole('region', { name: 'Save changes' })).toBeNull())

    // Scrolling back to the header and away again brings it back.
    setHeaderSaveVisible(context, true)
    setHeaderSaveVisible(context, false)
    expect(screen.getByRole('region', { name: 'Save changes' })).toBeTruthy()
  })
})
