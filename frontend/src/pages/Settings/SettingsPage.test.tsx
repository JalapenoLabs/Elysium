// Copyright © 2026 Jalapeno Labs

import type { Mock } from 'vitest'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { createMemoryRouter, Link, RouterProvider, useNavigate } from 'react-router'

// User interface
import { SettingsPage } from './SettingsPage'

// Misc
import { usePageSave } from '../../layout/page/pageSave'

type Observed = {
  callback: IntersectionObserverCallback
}

type Context = {
  observed: Observed[]
  // Resolves whether the save went through, as a form's save does.
  onSave: Mock<() => Promise<boolean>>
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
  onSave: () => Promise<boolean>
  // Where the form goes once saved, as an edit page going back to its list does.
  navigatesTo?: string
}

function ExampleForm(props: FormProps) {
  const navigate = useNavigate()
  const [ saved, setSaved ] = useState('')
  const [ value, setValue ] = useState('')

  async function save() {
    const isSaved = await props.onSave()
    if (isSaved) {
      setSaved(value)
      if (props.navigatesTo) {
        void navigate(props.navigatesTo)
      }
    }
    return isSaved
  }

  const onSubmit = usePageSave({ save, isDirty: value !== saved })

  return <form onSubmit={onSubmit}>
    <input aria-label='Value' value={value} onChange={(event) => setValue(event.target.value)} />
    <Link to='/elsewhere'>Elsewhere</Link>
  </form>
}

function renderPage(context: Context, navigatesTo?: string) {
  const router = createMemoryRouter([
    { path: '/settings', element: <p>Settings directory</p> },
    { path: '/settings/llms', element: <p>LLM list</p> },
    { path: '/elsewhere', element: <p>Somewhere else</p> },
    {
      path: '/settings/llms/1/edit',
      element: <SettingsPage
        title='Edit LLM'
        parents={[{ label: 'Manage LLMs', href: '/settings/llms' }]}
      >
        <ExampleForm onSave={context.onSave} navigatesTo={navigatesTo} />
      </SettingsPage>,
    },
  ], { initialEntries: [ '/settings/llms/1/edit' ]})
  render(<RouterProvider router={router} />)
  return router
}

function change() {
  fireEvent.change(screen.getByLabelText('Value'), { target: { value: 'changed' }})
}

describe('SettingsPage', () => {
  beforeEach<Context>((context) => {
    context.observed = []
    context.onSave = vi.fn<() => Promise<boolean>>().mockResolvedValue(true)
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

  it<Context>('puts the one Save in the header, enabled once something changed', async (context) => {
    renderPage(context)

    // Disabled, the button sits inside the trigger of the tooltip saying why.
    const save = screen
      .getAllByRole('button', { name: 'Save' })
      .find((element) => element.tagName === 'BUTTON')
    expect(save?.closest('header')).not.toBeNull()
    expect(save?.hasAttribute('disabled')).toBe(true)
    expect(save?.closest('[data-slot="tooltip-trigger"]')).not.toBeNull()

    change()
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(context.onSave).toHaveBeenCalledTimes(1))
  })

  it<Context>('presses Save on Ctrl+S, and never opens the browser\'s save dialog', async (context) => {
    renderPage(context)

    const untouched = fireEvent.keyDown(window, { key: 's', ctrlKey: true })
    expect(untouched).toBe(false)
    expect(context.onSave).not.toHaveBeenCalled()

    change()
    fireEvent.keyDown(window, { key: 's', ctrlKey: true })
    await waitFor(() => expect(context.onSave).toHaveBeenCalledTimes(1))
  })

  it<Context>('saves through the page when the form is submitted, as Enter does', async (context) => {
    renderPage(context)
    change()

    fireEvent.submit(screen.getByLabelText('Value').closest('form')!)
    await waitFor(() => expect(context.onSave).toHaveBeenCalledTimes(1))
  })

  it<Context>('goes back to the parent page on Esc', async (context) => {
    renderPage(context)

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(await screen.findByText('LLM list')).toBeTruthy()
  })

  it<Context>('asks before Esc leaves unsaved changes, and Keep editing stays', async (context) => {
    renderPage(context)
    change()

    fireEvent.keyDown(window, { key: 'Escape' })
    expect(await screen.findByRole('alertdialog')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Discard work' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Save and continue' })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Keep editing' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull())
    expect(screen.queryByText('LLM list')).toBeNull()
    expect(screen.getByLabelText('Value')).toBeTruthy()
  })

  it<Context>('blocks a link out while dirty, and Discard work follows it', async (context) => {
    renderPage(context)
    change()

    fireEvent.click(screen.getByRole('link', { name: 'Elsewhere' }))
    expect(await screen.findByRole('alertdialog')).toBeTruthy()
    expect(screen.queryByText('Somewhere else')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Discard work' }))
    expect(await screen.findByText('Somewhere else')).toBeTruthy()
    expect(context.onSave).not.toHaveBeenCalled()
  })

  it<Context>('lets a clean page be left without asking', async (context) => {
    renderPage(context)

    fireEvent.click(screen.getByRole('link', { name: 'Elsewhere' }))
    expect(await screen.findByText('Somewhere else')).toBeTruthy()
  })

  it<Context>('saves, then goes on to where the person was heading', async (context) => {
    // The form goes to its list after saving; the person asked for somewhere else.
    renderPage(context, '/settings/llms')
    change()

    fireEvent.click(screen.getByRole('link', { name: 'Elsewhere' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Save and continue' }))

    expect(await screen.findByText('Somewhere else')).toBeTruthy()
    expect(context.onSave).toHaveBeenCalledTimes(1)
  })

  it<Context>('stays when Save and continue could not save', async (context) => {
    context.onSave.mockResolvedValue(false)
    renderPage(context)
    change()

    fireEvent.click(screen.getByRole('link', { name: 'Elsewhere' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Save and continue' }))

    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull())
    expect(context.onSave).toHaveBeenCalledTimes(1)
    expect(screen.queryByText('Somewhere else')).toBeNull()
    expect((screen.getByLabelText('Value') as HTMLInputElement).value).toBe('changed')
  })

  it<Context>('lets the form go where it goes after its own save, without asking', async (context) => {
    renderPage(context, '/settings/llms')
    change()

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText('LLM list')).toBeTruthy()
    expect(screen.queryByRole('alertdialog')).toBeNull()
  })

  it<Context>('offers Save on a card while the header\'s is out of view, until dismissed', async (context) => {
    renderPage(context)
    change()

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
