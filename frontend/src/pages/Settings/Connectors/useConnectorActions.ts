// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { AccountKind, Connector } from '../../../api/routes/connectorRoutes'
import type { ConnectorAction } from './ConnectorRow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { checkRunLoaded, connectorDeleted, connectorUpserted } from '../../../store/connectorsSlice'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { accountNameOf } from './connectorPresentation'
import {
  deleteConnector,
  getConnectorMeetingCount,
  runConnectorChecks,
  syncConnector,
  updateConnector,
} from '../../../api/routes/connectorRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { toast } from '../../../toast/toast'
import { useConnectorKinds } from './useConnectorKinds'

// The removal dialog's switch that deletes the meetings the connector listed along with it.
const DELETE_MEETINGS_SWITCH = 'deleteMeetings'

// What removing a connector removes with it, by the account it owns.
const deleteBodyKeyByAccount = {
  atlassian: 'actions.deleteBody.atlassian',
  github: 'actions.deleteBody.github',
  browser: 'actions.deleteBody.browser',
  fireflies: 'actions.deleteBody.token',
  youtrack: 'actions.deleteBody.token',
} as const satisfies Record<AccountKind, ParseKeys<'connectors'>>

type Options = {
  // A run of checks started, which the caller follows.
  onRunStarted?: (runId: string) => void
  // The connector was removed, so a page about it can leave.
  onDeleted?: () => void
}

// Every action is tracked while it works. Delete works only while it counts the meetings its
// dialog offers to delete; the dialog then shows the removal working.
type RunningAction = ConnectorAction

// What the hub's gear menu does, shared by the list and a connector's page. Each action that
// is running is tracked by connector, so its button or menu trigger can show it working
// (`~/.claude/docs/ux.md`, Buttons). Connectors are independent, so one connector's action
// never waits for another's; pressing the same one again while it runs does nothing.
export function useConnectorActions(options: Options = {}) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const kinds = useConnectorKinds()
  const [ runningKeys, setRunningKeys ] = useState<ReadonlySet<string>>(() => new Set())

  async function attempt(action: () => Promise<void>, context: Record<string, unknown>) {
    try {
      await action()
    }
    catch (error) {
      console.debug('A connector action failed', { error, ...context })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }

  async function track(connector: Connector, action: RunningAction, work: () => Promise<void>) {
    const key = `${connector.id}:${action}`
    if (runningKeys.has(key)) {
      console.debug('A connector action is already running', { action, connectorId: connector.id })
      return
    }
    setRunningKeys((current) => new Set(current).add(key))
    try {
      await attempt(work, { action, connectorId: connector.id })
    }
    finally {
      setRunningKeys((current) => {
        const next = new Set(current)
        next.delete(key)
        return next
      })
    }
  }

  // How many meetings the connector listed, which its removal offers to delete with it: none for a
  // kind that provides no meetings, which is not asked. While the kinds are still loading it is
  // asked anyway, since the API counts none for such a kind. A count that cannot be read offers
  // nothing, and the removal still asks, leaving the meetings as they are.
  async function countMeetingsOf(connector: Connector) {
    const definition = kinds.kindOf(connector.kind)
    if (definition && !definition.provides.includes('meetings')) {
      return 0
    }
    try {
      const response = await getConnectorMeetingCount(connector.id)
      return response.count
    }
    catch (error) {
      console.debug('The meetings a connector listed could not be counted', { error, connectorId: connector.id })
      return 0
    }
  }

  function act(connector: Connector, action: ConnectorAction) {
    const handlers = {
      runChecks: () => track(connector, 'runChecks', async () => {
        const response = await runConnectorChecks(connector.id)
        dispatch(checkRunLoaded(response.run))
        options.onRunStarted?.(response.run.id)
      }),
      sync: () => track(connector, 'sync', async () => {
        await syncConnector(connector.id)
        toast.success(t('actions.syncing'))
      }),
      toggle: () => track(connector, 'toggle', async () => {
        const response = await updateConnector(connector.id, { isActive: !connector.isActive })
        dispatch(connectorUpserted(response.connector))
        toast.success(t('actions.saved'))
      }),
      delete: () => track(connector, 'delete', async () => {
        // Named as its row names it: the kind, and the account it reads as when it shows one.
        const kindName = t(`kinds.${connector.kind}.name`)
        const accountName = accountNameOf(connector)
        const name = accountName
          ? t('actions.deleteName', { kind: kindName, account: accountName })
          : kindName
        const meetingCount = await countMeetingsOf(connector)

        confirm({
          title: t('actions.deleteTitle', { name }),
          // Only a connector read through a browser is ever without its account, while it waits.
          message: t(deleteBodyKeyByAccount[connector.account?.kind ?? 'browser'], { name }),
          isIrreversible: true,
          confirmText: t('actions.delete'),
          // Its meetings stay unless asked; deleted with it, they are restored from the Meetings
          // page like any other, so the switch makes nothing irreversible.
          switches: meetingCount
            ? [{ id: DELETE_MEETINGS_SWITCH, label: t('actions.deleteMeetings', { count: meetingCount }) }]
            : undefined,
          // A meeting changed meanwhile is left as it is, so the toast names no count.
          onConfirm: (switchedOn) => attempt(async () => {
            const deleteMeetings = switchedOn.has(DELETE_MEETINGS_SWITCH)
            await deleteConnector(connector.id, { deleteMeetings })
            dispatch(connectorDeleted(connector.id))
            toast.success(deleteMeetings
              ? t('actions.deletedWithMeetings')
              : t('actions.deleted'))
            options.onDeleted?.()
          }, { action, connectorId: connector.id }),
        })
      }),
    } as const satisfies Record<ConnectorAction, () => unknown>
    void handlers[action]()
  }

  // Whether this action runs for the connector, or with no action named, whether any does.
  function isRunning(connector: Connector, action?: RunningAction) {
    if (action) {
      return runningKeys.has(`${connector.id}:${action}`)
    }
    for (const key of runningKeys) {
      if (key.startsWith(`${connector.id}:`)) {
        return true
      }
    }
    return false
  }

  return { act, isRunning } as const
}
