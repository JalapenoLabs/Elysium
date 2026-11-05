// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'

// Redux
import { createAppStore } from '../store'

// User interface
import { Button, useOverlayState } from '@heroui/react'
import { ListSearchField } from '../components/ListSearchField'
import { ElysiaHoursDialog } from '../elysia/ElysiaHoursDialog'
import { KnowledgeAskPage } from '../pages/Knowledge/Ask/KnowledgeAskPage'
import { PromptGate } from '../gates/PromptGate'
import { SnoozeDateModal } from '../pages/ActionItems/SnoozeDateModal'
import { PromptComposer } from '../pages/Coding/PromptComposer'

// Misc
import { usePrompt } from '../hooks/usePrompt'

// Issue #178: the hotkeys `~/.claude/docs/ux.md` asks for. Enter submits every dialog with a
// primary action, even one whose only field is a date (whose segments are not inputs, so the
// browser would never submit on its own); inside a text area Enter adds a line and Ctrl or Cmd
// with Enter submits, in dialogs and in the prompt composers alike. Ctrl+F focuses the page's
// own search unless focus is already in a text field.

const settings = vi.hoisted(() => ({ update: vi.fn() }))
vi.mock('../api/routes/elysiaRoutes', async (importOriginal) => ({
  ...await importOriginal<object>(),
  updateElysiaSettings: settings.update,
}))

const knowledge = vi.hoisted(() => ({ ask: vi.fn(), list: vi.fn() }))
vi.mock('../api/routes/knowledgeRoutes', async (importOriginal) => ({
  ...await importOriginal<object>(),
  askKnowledge: knowledge.ask,
  listKnowledgeAsks: knowledge.list,
}))

// jsdom has no ResizeObserver, which HeroUI's fields use for layout alone.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.stubGlobal('ResizeObserver', InertObserver)
  settings.update.mockReset()
  knowledge.ask.mockReset()
  knowledge.list.mockReset()
  knowledge.list.mockResolvedValue({ asks: []})
})

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('issue #178: Enter submits a dialog', () => {
  it('snoozes from the date\'s segments, which are not inputs', async () => {
    const onSnooze = vi.fn(async () => true)
    function Snooze() {
      const state = useOverlayState({ defaultOpen: true })
      return <SnoozeDateModal state={state} onSnooze={onSnooze} />
    }
    render(<Snooze />)

    const segments = await screen.findAllByRole('spinbutton')
    fireEvent.keyDown(segments[0], { key: 'Enter' })

    await waitFor(() => expect(onSnooze).toHaveBeenCalledTimes(1))
  })

  it('saves Elysia\'s hours from a switch', async () => {
    settings.update.mockResolvedValue({ elysia: {}})
    render(<Provider store={createAppStore()}>
      <ElysiaHoursDialog isOpen onOpenChange={vi.fn()} availability={[]} />
    </Provider>)

    fireEvent.keyDown(await screen.findByRole('switch'), { key: 'Enter' })

    await waitFor(() => expect(settings.update).toHaveBeenCalledWith({ availability: []}))
  })

  it('leaves Enter on Cancel to Cancel', async () => {
    render(<Provider store={createAppStore()}>
      <ElysiaHoursDialog isOpen onOpenChange={vi.fn()} availability={[]} />
    </Provider>)

    fireEvent.keyDown(await screen.findByRole('button', { name: 'Cancel' }), { key: 'Enter' })

    expect(settings.update).not.toHaveBeenCalled()
  })
})

describe('issue #178: Ctrl+Enter submits a text area', () => {
  it('adds a line in a multi-line prompt on Enter, and saves it on Ctrl+Enter', async () => {
    const onSubmit = vi.fn()
    function Ask() {
      const prompt = usePrompt()
      return <Button onPress={() => prompt({ title: 'Edit', label: 'Comment', isMultiline: true, onSubmit })}>
        <span>Ask</span>
      </Button>
    }
    render(<PromptGate>
      <Ask />
    </PromptGate>)
    act(() => screen.getByRole('button', { name: 'Ask' }).click())
    const field = await screen.findByLabelText(/Comment/)
    fireEvent.change(field, { target: { value: 'Two lines' }})

    fireEvent.keyDown(field, { key: 'Enter' })
    expect(onSubmit).not.toHaveBeenCalled()

    fireEvent.keyDown(field, { key: 'Enter', ctrlKey: true })
    await waitFor(() => expect(onSubmit).toHaveBeenCalledWith('Two lines'))
  })

  it('sends a session or Studio prompt on Ctrl+Enter or Cmd+Enter, never on Enter alone', async () => {
    const onSend = vi.fn(async () => true)
    render(<PromptComposer id='coding-session-1-prompt' onSend={onSend} />)
    const field = screen.getByRole('textbox')
    fireEvent.change(field, { target: { value: 'Fix the build' }})

    fireEvent.keyDown(field, { key: 'Enter' })
    expect(onSend).not.toHaveBeenCalled()

    fireEvent.keyDown(field, { key: 'Enter', metaKey: true })
    await waitFor(() => expect(onSend).toHaveBeenCalledWith('Fix the build'))
  })

  it('asks the knowledge base on Ctrl+Enter, never on Enter alone', async () => {
    knowledge.ask.mockReturnValue(new Promise(() => undefined))
    const router = createMemoryRouter([{ path: '/', element: <KnowledgeAskPage /> }])
    render(<Provider store={createAppStore()}>
      <RouterProvider router={router} />
    </Provider>)
    const field = screen.getByRole('textbox')
    fireEvent.change(field, { target: { value: 'When do we deploy?' }})

    fireEvent.keyDown(field, { key: 'Enter' })
    expect(knowledge.ask).not.toHaveBeenCalled()

    fireEvent.keyDown(field, { key: 'Enter', ctrlKey: true })
    await waitFor(() => expect(knowledge.ask).toHaveBeenCalledWith('When do we deploy?'))
  })
})

describe('issue #178: Ctrl+F focuses the page\'s search', () => {
  function SearchablePage() {
    const [ search, setSearch ] = useState('')
    return <div>
      <input aria-label='Notes' />
      <ListSearchField id='things-search' label='Search things' value={search} onChange={setSearch} />
    </div>
  }

  it('takes Ctrl+F from the browser\'s find while focus is outside a text field', () => {
    render(<SearchablePage />)

    const isHandled = !fireEvent.keyDown(document.body, { key: 'f', ctrlKey: true })

    expect(isHandled).toBe(true)
    expect(document.activeElement?.id).toBe('things-search')
  })

  it('leaves Ctrl+F to the browser while focus is in a text field', () => {
    render(<SearchablePage />)
    const notes = screen.getByLabelText('Notes')
    notes.focus()

    const isHandled = !fireEvent.keyDown(notes, { key: 'f', ctrlKey: true })

    expect(isHandled).toBe(false)
    expect(document.activeElement).toBe(notes)
  })
})
