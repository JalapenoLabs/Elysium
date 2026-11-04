// Copyright © 2026 Jalapeno Labs

import type { AgentGuidance, AgentGuidanceVersion } from '../../../api/routes/agentGuidanceRoutes'
import type { UserRole } from '../../../api/routes/userRoutes'

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { SWRConfig } from 'swr'

// Redux
import { createAppStore } from '../../../store'
import { meLoaded } from '../../../store/authSlice'

// User interface
import { AgentGuidancePage } from './AgentGuidancePage'

// Utility
import ky from 'ky'

// Misc
import {
  getAgentGuidance,
  restoreAgentGuidanceVersion,
  saveAgentGuidance,
} from '../../../api/routes/agentGuidanceRoutes'
import { makeUser } from '../../../testFixtures'
import { TestAriaRouter } from '../../../testRouter'

// The guidance's three routes are the network boundary.
vi.mock('../../../api/routes/agentGuidanceRoutes', async (importOriginal) => ({
  ...await importOriginal<object>(),
  getAgentGuidance: vi.fn(),
  saveAgentGuidance: vi.fn(),
  restoreAgentGuidanceVersion: vi.fn(),
}))

const RULES = 'Results may quote a coworker\'s mail and direct messages.'

function version(number: number, content: string): AgentGuidanceVersion {
  return {
    version: number,
    content,
    restoredFrom: null,
    createdBy: 'person',
    createdAt: '2026-11-04T09:00:00Z',
  }
}

// Version 2 current, over version 1.
function guidanceAt(current: AgentGuidanceVersion, ...older: AgentGuidanceVersion[]): AgentGuidance {
  return {
    current,
    versions: [ current, ...older ],
    rules: RULES,
    claudeMd: `<!-- Elysium's agent guidance, version ${current.version}. -->\n${current.content}\n`,
    maxCharacters: 40,
  }
}

// What ky throws for a refusal the API names by its code.
function refusal(status: number, code: string) {
  return ky('http://localhost/api/v1/agent-guidance', {
    retry: 0,
    fetch: () => Promise.resolve(Response.json({ code, message: 'refused' }, { status })),
  }).catch((error: unknown) => error)
}

function renderAs(role: UserRole) {
  const store = createAppStore()
  store.dispatch(meLoaded({
    user: makeUser({ role }),
    hasSecondFactor: true,
    mfaEnrollmentRequired: false,
    authenticatorNickname: null,
  }))
  // A data router, since the page guards its unsaved changes with a blocker.
  const router = createMemoryRouter([{
    path: '*',
    element: <TestAriaRouter>
      <AgentGuidancePage />
    </TestAriaRouter>,
  }], { initialEntries: [ '/settings/agent-guidance' ]})
  // A fresh cache per test, so one test's guidance never answers another's.
  return render(<SWRConfig value={{ provider: () => new Map() }}>
    <Provider store={store}>
      <RouterProvider router={router} />
    </Provider>
  </SWRConfig>)
}

// jsdom has neither observer; the settings frame and HeroUI's fields watch elements.
class InertObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

async function guidanceField() {
  return await screen.findByRole('textbox', { name: 'Guidance' }) as HTMLTextAreaElement
}

describe('AgentGuidancePage', () => {
  beforeEach(() => {
    vi.stubGlobal('ResizeObserver', InertObserver)
    vi.stubGlobal('IntersectionObserver', InertObserver)
    vi.mocked(getAgentGuidance).mockResolvedValue(
      guidanceAt(version(2, 'Search first.'), version(1, 'Ask first.')),
    )
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.mocked(getAgentGuidance).mockReset()
    vi.mocked(saveAgentGuidance).mockReset()
    vi.mocked(restoreAgentGuidanceVersion).mockReset()
  })

  it('lets an admin edit the guidance and saves it against the version it was edited from', async () => {
    vi.mocked(saveAgentGuidance).mockResolvedValue(
      guidanceAt(version(3, 'Search the knowledge base first.'), version(2, 'Search first.')),
    )
    renderAs('admin')

    const field = await guidanceField()
    expect(field.value).toBe('Search first.')
    expect(screen.getByText(RULES)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Copy CLAUDE.md block' })).toBeTruthy()
    // Restore is offered on the older version alone: the current one is what agents read.
    expect(screen.getAllByRole('button', { name: 'Restore this version' })).toHaveLength(1)

    fireEvent.change(field, { target: { value: 'Search the knowledge base first.' }})
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveAgentGuidance).toHaveBeenCalledWith('Search the knowledge base first.', 2))
  })

  it('keeps the admin\'s text when someone saved meanwhile, and saves over it only when asked again', async () => {
    renderAs('admin')
    const field = await guidanceField()
    vi.mocked(saveAgentGuidance).mockRejectedValueOnce(await refusal(409, 'guidance_changed'))
    vi.mocked(getAgentGuidance).mockResolvedValue(
      guidanceAt(version(3, 'Ask in #platform.'), version(2, 'Search first.')),
    )

    fireEvent.change(field, { target: { value: 'Search first, then ask.' }})
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(saveAgentGuidance).toHaveBeenCalledTimes(1))
    expect(saveAgentGuidance).toHaveBeenLastCalledWith('Search first, then ask.', 2)
    await screen.findByText('Version 3')
    expect(field.value).toBe('Search first, then ask.')

    vi.mocked(saveAgentGuidance).mockResolvedValue(
      guidanceAt(version(4, 'Search first, then ask.'), version(3, 'Ask in #platform.')),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(saveAgentGuidance).toHaveBeenLastCalledWith('Search first, then ask.', 3))
  })

  it('holds Save while the guidance is longer than every agent reads whole', async () => {
    renderAs('admin')
    const field = await guidanceField()

    fireEvent.change(field, { target: { value: 'x'.repeat(41) }})
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('At most 40 characters, so every agent reads all of it.')).toBeTruthy()
    expect(saveAgentGuidance).not.toHaveBeenCalled()
  })

  it('shows a member the guidance to read and the block to copy, with nothing to change', async () => {
    renderAs('member')

    const field = await guidanceField()
    expect(field.value).toBe('Search first.')
    expect(field.readOnly).toBe(true)
    expect(screen.getByRole('button', { name: 'Copy CLAUDE.md block' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Save' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Restore this version' })).toBeNull()
  })

  it('tells a guest the knowledge base is for members, and asks for nothing', () => {
    renderAs('guest')

    expect(screen.getByText('The knowledge base is for the company\'s members.')).toBeTruthy()
    expect(getAgentGuidance).not.toHaveBeenCalled()
  })
})
