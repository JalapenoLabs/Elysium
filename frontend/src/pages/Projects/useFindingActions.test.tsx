// Copyright © 2026 Jalapeno Labs

import type { PromptOptions } from '../../gates/PromptGate'
import type { ToastOptions } from '../../toast/toast'

// Core
import { act, render } from '@testing-library/react'
import { HTTPError } from 'ky'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { makeFinding } from '../../testFixtures'
import { useFindingActions } from './useFindingActions'

// The finding requests are the network boundary; toasts and the prompt are watched for what the
// person reads.
const routes = vi.hoisted(() => ({
  acceptFinding: vi.fn(),
  dismissFinding: vi.fn(),
}))

vi.mock('../../api/routes/findingRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../api/routes/findingRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  warning: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../toast/toast', () => ({ toast: toasts }))

const prompt = vi.hoisted(() => vi.fn())

vi.mock('../../hooks/usePrompt', () => ({ usePrompt: () => prompt }))

const FINDING = makeFinding({ id: 'retry', projectId: 'launch', title: 'Retry forever' })

function decided() {
  const response = new Response(null, { status: 409 })
  const error = new HTTPError(response, new Request('http://localhost/'), {} as never)
  error.data = { code: 'finding_decided', message: 'someone already decided this finding' }
  return error
}

function renderActions() {
  const captured: { actions?: ReturnType<typeof useFindingActions> } = {}
  function Harness() {
    captured.actions = useFindingActions()
    return null
  }
  render(<MemoryRouter>
    <Harness />
  </MemoryRouter>)
  if (!captured.actions) {
    throw new Error('the hook did not render')
  }
  return captured.actions
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('useFindingActions accept', () => {
  it('says where its issue files and offers the batch it proposed', async () => {
    routes.acceptFinding.mockResolvedValue({
      finding: { ...FINDING, state: 'accepted' },
      itemId: 'item',
      batchId: 'batch',
      issue: { provider: 'jira', destination: 'ELY' },
      issueRefusal: null,
    })
    const actions = renderActions()

    await act(() => actions.accept(FINDING))

    expect(routes.acceptFinding).toHaveBeenCalledWith('retry')
    expect(toasts.success).toHaveBeenCalledWith('Accepted Retry forever', expect.anything())
    const options: ToastOptions = toasts.success.mock.lastCall?.[1]
    expect(options.description).toBe('Its issue files in Jira ELY, and its batch waits for your approval.')
    expect(options.action?.label).toBe('Review batch')
  })

  it('warns, and keeps the warning, when the project has nowhere to file', async () => {
    routes.acceptFinding.mockResolvedValue({
      finding: { ...FINDING, state: 'accepted' },
      itemId: 'item',
      batchId: 'batch',
      issue: null,
      issueRefusal: 'this project names no tracker',
    })
    const actions = renderActions()

    await act(() => actions.accept(FINDING))

    expect(toasts.success).not.toHaveBeenCalled()
    expect(toasts.warning).toHaveBeenCalledWith('Accepted Retry forever without an issue', expect.anything())
  })

  it('says someone else decided it first', async () => {
    routes.acceptFinding.mockRejectedValue(decided())
    const actions = renderActions()

    await act(() => actions.accept(FINDING))

    expect(toasts.danger).toHaveBeenCalledWith('Someone already decided this finding.')
  })
})

describe('useFindingActions dismiss', () => {
  it('asks first, then dismisses with the reason given, or none', async () => {
    routes.dismissFinding.mockResolvedValue({ finding: { ...FINDING, state: 'dismissed' }})
    const actions = renderActions()

    act(() => actions.dismiss(FINDING))

    expect(routes.dismissFinding).not.toHaveBeenCalled()
    const options: PromptOptions = prompt.mock.lastCall?.[0]
    expect(options.title).toBe('Dismiss Retry forever?')
    expect(options.isOptional).toBe(true)

    await act(() => options.onSubmit('The provider never answers 429.'))
    expect(routes.dismissFinding).toHaveBeenLastCalledWith('retry', 'The provider never answers 429.')
    expect(toasts.success).toHaveBeenCalledWith('Dismissed Retry forever')

    await act(() => options.onSubmit(''))
    expect(routes.dismissFinding).toHaveBeenLastCalledWith('retry', null)
  })

  it('closes the dialog when someone else decided it, and keeps it open on any other failure', async () => {
    const actions = renderActions()
    act(() => actions.dismiss(FINDING))
    const options: PromptOptions = prompt.mock.lastCall?.[0]

    routes.dismissFinding.mockRejectedValueOnce(decided())
    await expect(options.onSubmit('')).resolves.toBeUndefined()

    routes.dismissFinding.mockRejectedValueOnce(new Error('offline'))
    await expect(options.onSubmit('')).rejects.toThrow('offline')
  })
})
