// Copyright © 2026 Jalapeno Labs

import type { ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'
import type { ConfirmOptions } from '../../../gates/ConfirmGate'

// Core
import { act, render, waitFor } from '@testing-library/react'
import { Provider } from 'react-redux'
import { SWRConfig } from 'swr'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Redux
import { createAppStore } from '../../../store'

// Misc
import { ConfirmContext } from '../../../gates/ConfirmGate'
import { makeConnector } from '../../../testFixtures'
import { useConnectorActions } from './useConnectorActions'

// The connector requests are the network boundary; toasts are watched for what they say.
const routes = vi.hoisted(() => ({
  listConnectorKinds: vi.fn(),
  getConnectorMeetingCount: vi.fn(),
  deleteConnector: vi.fn(),
}))

vi.mock('../../../api/routes/connectorRoutes', async (importOriginal) => ({
  ...await importOriginal<typeof import('../../../api/routes/connectorRoutes')>(),
  ...routes,
}))

const toasts = vi.hoisted(() => ({
  success: vi.fn(),
  danger: vi.fn(),
}))

vi.mock('../../../toast/toast', () => ({ toast: toasts }))

function definition(
  kind: ConnectorKindDefinition['kind'],
  provides: ConnectorKindDefinition['provides'],
): ConnectorKindDefinition {
  return {
    kind,
    provides,
    scope: 'personal',
    account: 'fireflies',
    startUrl: null,
    content: [],
    checks: [],
    replies: { through: 'never' },
  }
}

const FIREFLIES = makeConnector({
  id: 'fireflies',
  kind: 'fireflies',
  scope: 'personal',
  account: { id: 'key', kind: 'fireflies', name: 'ada@example.com' },
})
const JIRA = makeConnector({ id: 'jira' })

// Holds the actions in a harness, with the app's confirmation dialog replaced by a spy that shows
// what it was asked, once the kinds have loaded.
async function renderActions() {
  const confirm = vi.fn<(options: ConfirmOptions) => void>()
  const captured: { actions?: ReturnType<typeof useConnectorActions> } = {}
  function Harness() {
    captured.actions = useConnectorActions()
    return null
  }
  render(<Provider store={createAppStore()}>
    <SWRConfig value={{ provider: () => new Map() }}>
      <ConfirmContext.Provider value={confirm}>
        <Harness />
      </ConfirmContext.Provider>
    </SWRConfig>
  </Provider>)
  await waitFor(() => expect(routes.listConnectorKinds).toHaveBeenCalled())
  // The kinds land in SWR's cache on the next render.
  await act(async () => {})
  return { confirm, captured }
}

beforeEach(() => {
  vi.clearAllMocks()
  routes.listConnectorKinds.mockResolvedValue({
    kinds: [ definition('fireflies', [ 'meetings' ]), definition('jira', [ 'issues' ]) ],
    accounts: [],
  })
  routes.getConnectorMeetingCount.mockResolvedValue({ count: 13 })
  routes.deleteConnector.mockResolvedValue(undefined)
})

// #258: removing the Fireflies connector left its 13 meetings behind, with no way to delete them.
describe('useConnectorActions delete', () => {
  it('offers to delete the meetings a connector read, saying how many, and deletes them only when asked', async () => {
    const { confirm, captured } = await renderActions()

    await act(async () => captured.actions?.act(FIREFLIES, 'delete'))

    await waitFor(() => expect(confirm).toHaveBeenCalled())
    expect(routes.getConnectorMeetingCount).toHaveBeenCalledWith('fireflies')
    const asked = confirm.mock.lastCall?.[0]
    expect(asked?.isIrreversible).toBe(true)
    // Off unless switched on, as every switch in the dialog starts.
    expect(asked?.switches).toEqual([{ id: 'deleteMeetings', label: 'Also delete the 13 meetings it read' }])

    await act(async () => asked?.onConfirm(new Set([ 'deleteMeetings' ])))
    expect(routes.deleteConnector).toHaveBeenCalledWith('fireflies', { deleteMeetings: true })
    expect(toasts.success).toHaveBeenCalledWith('Connector removed, with its meetings')

    await act(async () => asked?.onConfirm(new Set()))
    expect(routes.deleteConnector).toHaveBeenLastCalledWith('fireflies', { deleteMeetings: false })
    expect(toasts.success).toHaveBeenLastCalledWith('Connector removed')
  })

  it('offers no switch for a kind that reads no meetings, nor for a connector that read none', async () => {
    const { confirm, captured } = await renderActions()

    await act(async () => captured.actions?.act(JIRA, 'delete'))
    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(1))
    expect(routes.getConnectorMeetingCount).not.toHaveBeenCalled()
    expect(confirm.mock.lastCall?.[0].switches).toBeUndefined()

    routes.getConnectorMeetingCount.mockResolvedValue({ count: 0 })
    await act(async () => captured.actions?.act(FIREFLIES, 'delete'))
    await waitFor(() => expect(confirm).toHaveBeenCalledTimes(2))
    expect(confirm.mock.lastCall?.[0].switches).toBeUndefined()
  })

  it('still asks when the meetings cannot be counted, leaving them as they are', async () => {
    routes.getConnectorMeetingCount.mockRejectedValue(new Error('offline'))
    const { confirm, captured } = await renderActions()

    await act(async () => captured.actions?.act(FIREFLIES, 'delete'))

    await waitFor(() => expect(confirm).toHaveBeenCalled())
    expect(confirm.mock.lastCall?.[0].switches).toBeUndefined()
  })
})
