// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../store'

// User interface
import { Button } from '@heroui/react'
import { ActionItemForm } from '../pages/ActionItems/ActionItemForm'
import { CreateSessionForm } from '../pages/Coding/CreateSessionForm'
import { InitiativeForm } from '../pages/Initiatives/InitiativeForm'
import { ProjectForm } from '../pages/Projects/ProjectForm'
import { EnvironmentVariableForm } from '../pages/Settings/Environment/EnvironmentVariableForm'
import { LearningCategoryForm } from '../pages/Settings/LearningCategories/LearningCategoryForm'
import { LearningForm } from '../pages/Settings/Learnings/LearningForm'
import { LlmForm } from '../pages/Settings/Llms/LlmForm'
import { MailDomainForm } from '../pages/Settings/Email/MailDomainForm'
import { MailServerForm } from '../pages/Settings/Email/MailServerForm'
import { StorageLocationForm } from '../pages/Settings/Storage/StorageLocationForm'
import { CreateStudioItemForm } from '../pages/Studio/CreateStudioItemForm'
import { ConfirmGate } from '../gates/ConfirmGate'
import { PromptGate } from '../gates/PromptGate'
import { Page } from '../layout/page/Page'

// Misc
import { useConfirm } from '../hooks/useConfirm'
import { usePrompt } from '../hooks/usePrompt'
import { TestAriaRouter } from '../testRouter'

// Issue #167: the first field of every form is focused when it opens, with one shared hook
// rather than `autoFocus`, which is unreliable when a form animates in. Every input has a
// stable id, and every text input says what the browser may autofill (`off` for app data).
// The hook is a ref callback, because dialogs such as the prompt gate are mounted once for
// the whole app and only render their field while open: a mount effect would run before the
// field exists and never again.

// Every loader reports its data as loaded, so the forms render without the API.
vi.mock('../hooks/useServerData', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  return Object.fromEntries(Object.keys(actual).map((name) => [ name, () => 'loaded' ]))
})

// jsdom has neither observer, which the page and HeroUI's fields use for layout alone.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

// The workspace's own frame around a form: the store, a data router for the page's leave
// guard, and React Aria's router for the links below a /new form.
function renderInPage(form: ReactNode) {
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>
      <Page title='Form'>{form}</Page>
    </TestAriaRouter>,
  }])
  return render(<Provider store={createAppStore()}>
    <RouterProvider router={router} />
  </Provider>)
}

// What a person can type into or choose with. React Aria's hidden copies for native form
// submission are not shown to anyone.
function visibleFields(container: HTMLElement) {
  const fields = container.querySelectorAll<HTMLElement>('input, textarea, select')
  return [ ...fields ].filter((field) => field.getAttribute('type') !== 'hidden'
    && !field.closest('[aria-hidden="true"], [data-react-aria-prevent-focus]'))
}

const GENERATED_ID = /^react-aria|[:«»]/

const TEXT_INPUT_TYPES = new Set([ 'text', 'email', 'url', 'search', 'password', 'tel' ])

const noop = () => {}

type NewForm = {
  name: string
  renderForm: () => ReactNode
  firstFieldId: string
}

const newForms: NewForm[] = [
  {
    name: 'New item',
    renderForm: () => <ActionItemForm initialProjectIds={[]} initialInitiativeIds={[]} onSaved={noop} />,
    firstFieldId: 'action-item-title',
  },
  {
    name: 'New initiative',
    renderForm: () => <InitiativeForm initialProjectIds={[]} onSaved={noop} />,
    firstFieldId: 'initiative-name',
  },
  { name: 'New project', renderForm: () => <ProjectForm onSaved={noop} />, firstFieldId: 'project-name' },
  {
    name: 'New session',
    renderForm: () => <CreateSessionForm actionItem={null} />,
    firstFieldId: 'coding-session-title',
  },
  { name: 'New Studio item', renderForm: () => <CreateStudioItemForm />, firstFieldId: 'studio-item-prompt' },
  { name: 'New learning', renderForm: () => <LearningForm learning={null} />, firstFieldId: 'learning-title' },
  {
    name: 'New learning category',
    renderForm: () => <LearningCategoryForm category={null} onSaved={noop} />,
    firstFieldId: 'learning-category-label',
  },
  {
    name: 'New environment variable',
    renderForm: () => <EnvironmentVariableForm variable={null} onSaved={noop} />,
    firstFieldId: 'environment-variable-key',
  },
  {
    name: 'New storage location',
    renderForm: () => <StorageLocationForm location={null} onSaved={noop} />,
    firstFieldId: 'storage-location-name',
  },
  {
    name: 'New LLM',
    renderForm: () => <LlmForm type='claude-api-token' llm={null} onSaved={noop} />,
    firstFieldId: 'llm-name',
  },
  { name: 'New mail server', renderForm: () => <MailServerForm />, firstFieldId: 'mail-server-domain' },
  { name: 'New mail domain', renderForm: () => <MailDomainForm />, firstFieldId: 'mail-domain-name' },
]

describe('issue #167', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.stubGlobal('IntersectionObserver', InertObserver)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it.each(newForms)('$name focuses its first field as it opens', async (newForm) => {
    renderInPage(newForm.renderForm())

    await waitFor(() => expect(document.activeElement?.id).toBe(newForm.firstFieldId))
  })

  it.each(newForms)('$name gives every field a stable id of its own', (newForm) => {
    const { container } = renderInPage(newForm.renderForm())

    const fields = visibleFields(container)
    expect(fields.length).toBeGreaterThan(0)
    // React Aria makes up an id for a field given none (`react-aria…` or React's `«r1»`),
    // which changes from render to render, so only a written one counts.
    const missing = fields
      .filter((field) => !field.id || GENERATED_ID.test(field.id))
      .map((field) => field.outerHTML)
    expect(missing).toEqual([])
  })

  it.each(newForms)('$name says what the browser may autofill in every text input', (newForm) => {
    const { container } = renderInPage(newForm.renderForm())

    const textInputs = visibleFields(container).filter((field) => field instanceof HTMLInputElement
      && TEXT_INPUT_TYPES.has(field.type))
    const missing = textInputs.filter((field) => !field.getAttribute('autocomplete')).map((field) => field.id)
    expect(missing).toEqual([])
  })

  it('focuses the prompt gate\'s field each time it opens, though the gate mounts once', async () => {
    function AskTwice() {
      const prompt = usePrompt()
      return <Button onPress={() => prompt({ title: 'Rename', label: 'Name', onSubmit: noop })}>
        <span>Ask</span>
      </Button>
    }
    render(<PromptGate>
      <AskTwice />
    </PromptGate>)

    // Opened twice: the second opening is the one a mount effect would miss.
    for (const attempt of [ 'first', 'second' ]) {
      act(() => screen.getByRole('button', { name: 'Ask' }).click())
      const field = await screen.findByLabelText(/Name/)
      await waitFor(() => expect(document.activeElement, attempt).toBe(field))
      expect(field.id).toBe('prompt-answer')
      act(() => screen.getByRole('button', { name: 'Cancel' }).click())
      await waitFor(() => expect(screen.queryByLabelText(/Name/)).toBeNull())
    }
  })

  it('focuses a safe confirmation\'s button, so Enter confirms it', async () => {
    function AskToConfirm() {
      const confirm = useConfirm()
      return <Button onPress={() => confirm({ title: 'Archive', message: 'Archive it?', onConfirm: noop })}>
        <span>Ask</span>
      </Button>
    }
    render(<ConfirmGate>
      <AskToConfirm />
    </ConfirmGate>)

    act(() => screen.getByRole('button', { name: 'Ask' }).click())

    const confirmButton = await screen.findByRole('button', { name: 'Confirm' })
    await waitFor(() => expect(document.activeElement).toBe(confirmButton))
  })
})
