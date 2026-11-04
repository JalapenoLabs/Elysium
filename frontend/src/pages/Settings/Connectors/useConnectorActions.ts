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
  runConnectorChecks,
  syncConnector,
  updateConnector,
} from '../../../api/routes/connectorRoutes'
import { useConfirm } from '../../../hooks/useConfirm'
import { toast } from '../../../toast/toast'

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

// The actions that run on their own; Delete asks first, and its dialog shows it working.
type RunningAction = Exclude<ConnectorAction, 'delete'>

// What the hub's gear menu does, shared by the list and a connector's page. Each action that
// is running is tracked by connector, so its button or menu trigger can show it working
// (`~/.claude/docs/ux.md`, Buttons). Connectors are independent, so one connector's action
// never waits for another's; pressing the same one again while it runs does nothing.
export function useConnectorActions(options: Options = {}) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
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
      delete: () => {
        // Named as its row names it: the kind, and the account it reads as when it shows one.
        const kindName = t(`kinds.${connector.kind}.name`)
        const accountName = accountNameOf(connector)
        const name = accountName
          ? t('actions.deleteName', { kind: kindName, account: accountName })
          : kindName

        confirm({
          title: t('actions.deleteTitle', { name }),
          // Only a connector read through a browser is ever without its account, while it waits.
          message: t(deleteBodyKeyByAccount[connector.account?.kind ?? 'browser'], { name }),
          isIrreversible: true,
          confirmText: t('actions.delete'),
          onConfirm: () => attempt(async () => {
            await deleteConnector(connector.id)
            dispatch(connectorDeleted(connector.id))
            toast.success(t('actions.deleted'))
            options.onDeleted?.()
          }, { action, connectorId: connector.id }),
        })
      },
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
