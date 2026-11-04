// Copyright © 2026 Jalapeno Labs

import type { Mock } from 'vitest'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { useState } from 'react'
import { createMemoryRouter, RouterProvider } from 'react-router'

// User interface
import { Page } from './Page'
import { PageCreateFooter } from './PageCreateFooter'

// Misc
import { usePageSave } from './pageSave'
import { TestAriaRouter } from '../../testRouter'

type Context = {
  // Resolves whether the create went through, as a form's save does.
  onSave: Mock<() => Promise<boolean>>
  // What each watched element reports to, as scrolling would.
  observers: Map<Element, IntersectionObserverCallback>
}

// Reports an element as in or out of view.
function setVisible(context: Context, element: Element, isVisible: boolean) {
  const callback = context.observers.get(element)
  if (!callback) {
    throw new Error('the element is not observed')
  }
  act(() => {
    callback([ { isIntersecting: isVisible } as IntersectionObserverEntry ], {} as IntersectionObserver)
  })
}

type FormProps = {
  onSave: () => Promise<boolean>
  canSaveUnchanged?: boolean
}

function NewThingForm(props: FormProps) {
  const [ value, setValue ] = useState('')
  const onSubmit = usePageSave({
    save: props.onSave,
    isDirty: Boolean(value),
    label: 'Add thing',
    canSaveUnchanged: props.canSaveUnchanged,
  })

  return <form onSubmit={onSubmit}>
    <input aria-label='Name' value={value} onChange={(event) => setValue(event.target.value)} />
    <PageCreateFooter />
  </form>
}

function renderNewPage(context: Context, canSaveUnchanged?: boolean) {
  const router = createMemoryRouter([
    { path: '/things', element: <p>Every thing</p> },
    {
      path: '/things/new',
      element: <TestAriaRouter>
        <Page title='New thing' parents={[{ label: 'Things', href: '/things' }]}>
          <NewThingForm onSave={context.onSave} canSaveUnchanged={canSaveUnchanged} />
        </Page>
      </TestAriaRouter>,
    },
  ], { initialEntries: [ '/things/new' ]})
  render(<RouterProvider router={router} />)
  return router
}

// The header's Create and the footer's, by where they sit.
function createButtons() {
  const footer = screen.getByLabelText('Name').closest('form')!
  const isButton = (element: HTMLElement) => element.tagName === 'BUTTON'
  return {
    header: within(screen.getByRole('banner')).getAllByRole('button', { name: 'Add thing' }).find(isButton),
    footer: within(footer).getAllByRole('button', { name: 'Add thing' }).find(isButton),
  }
}

describe('PageCreateFooter', () => {
  beforeEach<Context>((context) => {
    context.onSave = vi.fn<() => Promise<boolean>>().mockResolvedValue(true)
    context.observers = new Map()
    vi.stubGlobal('IntersectionObserver', class {
      private callback: IntersectionObserverCallback
      constructor(callback: IntersectionObserverCallback) {
        this.callback = callback
      }
      observe(element: Element) {
        context.observers.set(element, this.callback)
      }
      disconnect() {}
    })
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it<Context>('mirrors the header\'s Create: the same label, disabled for the same reason', (context) => {
    renderNewPage(context)

    const { header, footer } = createButtons()
    expect(header?.hasAttribute('disabled')).toBe(true)
    expect(footer?.hasAttribute('disabled')).toBe(true)
    // Disabled, each sits inside the trigger of the tooltip saying why.
    expect(footer?.closest('[data-slot="tooltip-trigger"]')).not.toBeNull()

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Lamp' }})
    expect(createButtons().header?.hasAttribute('disabled')).toBe(false)
    expect(createButtons().footer?.hasAttribute('disabled')).toBe(false)
  })

  it<Context>('creates through the page\'s one save, once', async (context) => {
    renderNewPage(context)
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Lamp' }})

    fireEvent.click(createButtons().footer!)
    await waitFor(() => expect(context.onSave).toHaveBeenCalledTimes(1))
  })

  it<Context>('goes Back with a real link to the parent page, asking first with unsaved work', async (context) => {
    const router = renderNewPage(context)

    const back = within(screen.getByLabelText('Name').closest('form')!).getByRole('link', { name: 'Back' })
    expect(back.getAttribute('href')).toBe('/things')

    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Lamp' }})
    fireEvent.click(back)
    expect(await screen.findByText('Unsaved changes')).toBeTruthy()
    expect(router.state.location.pathname).toBe('/things/new')
  })

  it<Context>('can create with nothing typed when the page needs no input', async (context) => {
    const router = renderNewPage(context, true)

    fireEvent.click(createButtons().footer!)
    await waitFor(() => expect(context.onSave).toHaveBeenCalledTimes(1))

    // Nothing was typed, so there is nothing to lose by leaving.
    fireEvent.click(screen.getByRole('link', { name: 'Back' }))
    await waitFor(() => expect(router.state.location.pathname).toBe('/things'))
  })

  it<Context>('never adds the floating Save while the Create below the form is in view', (context) => {
    renderNewPage(context)
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'Lamp' }})

    const footer = document.querySelector('.page-create-footer')!
    // The page watches two elements: the header's Save and the footer.
    const headerSave = [ ...context.observers.keys() ].find((element) => element !== footer)!

    setVisible(context, headerSave, false)
    setVisible(context, footer, true)
    expect(screen.queryByRole('region', { name: 'Save changes' })).toBeNull()

    setVisible(context, footer, false)
    expect(screen.getByRole('region', { name: 'Save changes' })).toBeTruthy()
  })
})
