// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { useState } from 'react'
import { createPortal } from 'react-dom'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'

// Redux
import { createAppStore } from '../store'
import { meLoaded } from '../store/authSlice'

// User interface
import { LuRefreshCw } from 'react-icons/lu'
import { Page } from '../layout/page/Page'
import { SettingsPage } from '../pages/Settings/SettingsPage'

// Misc
import { usePageSave } from '../layout/page/pageSave'
import { usePageToolsContainer } from '../layout/page/pageTools'
import { makeUser } from '../testFixtures'
import { TestAriaRouter } from '../testRouter'

// Issue #191: in Firefox, a page whose header got its Save or actions after the first render
// never showed them. The header drew its right side on every page and hid it with a `:has()`
// rule while only an empty slot was inside, and Firefox does not re-check that rule as buttons
// arrive. The header now draws its right side only while React knows something is in it: a
// Save, an action, a primary action, tools, or a list that claims the slot. jsdom evaluates no
// `:has()`, so these tests pin that React state, which is what every browser renders from.

// jsdom has neither observer; the page uses them only to place the floating Save card and to
// lay out HeroUI's fields.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal('IntersectionObserver', InertObserver)
  vi.stubGlobal('ResizeObserver', InertObserver)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function renderPage(element: ReactNode) {
  // A data router, as the app has, since a page guards its unsaved changes with a blocker.
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>{element}</TestAriaRouter>,
  }], { initialEntries: [ '/settings/example' ]})
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser(),
    hasSecondFactor: false,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  return render(<SWRConfig value={{ provider: () => new Map() }}>
    <Provider store={store}>
      <RouterProvider router={router} />
    </Provider>
  </SWRConfig>)
}

function titlebarEnd() {
  return screen.getByRole('banner').querySelector('.page-actions')
}

// A form that registers the page's Save only once its data is here, as a settings page that
// loads its record does.
function LoadingForm() {
  const [ isLoaded, setIsLoaded ] = useState(false)
  if (!isLoaded) {
    return <button type='button' onClick={() => setIsLoaded(true)}>Finish loading</button>
  }
  return <LoadedForm />
}

function LoadedForm() {
  const onSubmit = usePageSave({ save: async () => true, isDirty: true })
  return <form onSubmit={onSubmit} />
}

// Stands in for a list table: puts its controls where `controls={{ container }}` would.
function ListWithControls() {
  const container = usePageToolsContainer()
  return container && createPortal(<button type='button'>Filters</button>, container)
}

describe('issue #191: the header\'s right side follows what it is given', () => {
  it('shows a Save registered after the page\'s first render', () => {
    renderPage(<SettingsPage title='Example'>
      <LoadingForm />
    </SettingsPage>)

    // Nothing to hold yet, so there is no right side to hide.
    expect(titlebarEnd()).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Finish loading' }))

    const end = titlebarEnd()
    expect(end).not.toBeNull()
    expect(within(end as HTMLElement).getByRole('button', { name: 'Save' })).toBeTruthy()
  })

  it('shows actions given after the page\'s first render', () => {
    function LateActions() {
      const [ hasAction, setHasAction ] = useState(false)
      return <Page
        title='Example'
        actions={hasAction
          ? [{ id: 'sync', label: 'Sync now', icon: LuRefreshCw, onAction: () => {} }]
          : []}
      >
        <button type='button' onClick={() => setHasAction(true)}>Finish loading</button>
      </Page>
    }
    renderPage(<LateActions />)

    expect(titlebarEnd()).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Finish loading' }))

    expect(within(titlebarEnd() as HTMLElement).getByRole('button', { name: 'Sync now' })).toBeTruthy()
  })

  it('draws the list slot while a list claims it, and drops it when the list goes', () => {
    function LateList() {
      const [ hasList, setHasList ] = useState(false)
      return <Page title='Example'>
        <button type='button' onClick={() => setHasList((value) => !value)}>Toggle list</button>
        {hasList && <ListWithControls />}
      </Page>
    }
    renderPage(<LateList />)

    expect(titlebarEnd()).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Toggle list' }))
    const end = titlebarEnd() as HTMLElement
    expect(end.querySelector('.page-tools-slot')).not.toBeNull()
    expect(within(end).getByRole('button', { name: 'Filters' })).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Toggle list' }))
    expect(titlebarEnd()).toBeNull()
  })
})
