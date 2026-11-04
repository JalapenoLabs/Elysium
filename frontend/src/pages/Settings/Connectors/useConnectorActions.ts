// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { AccountKind, Connector } from '../../../api/routes/connectorRoutes'
import type { ConnectorAction } from './ConnectorRow'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { checkRunLoaded, connectorDeleted, connectorUpserted } from '../../../store/connectorsSlice'

// User interface
import { toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import {
  deleteConnector,
  runConnectorChecks,
  syncConnector,
  updateConnector,
} from '../../../api/routes/connectorRoutes'
import { useConfirm } from '../../../hooks/useConfirm'

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

// What the hub's gear menu does, shared by the list and a connector's page.
export function useConnectorActions(options: Options = {}) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()

  async function attempt(action: () => Promise<void>, context: Record<string, unknown>) {
    try {
      await action()
    }
    catch (error) {
      console.debug('A connector action failed', { error, ...context })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }

  return function act(connector: Connector, action: ConnectorAction) {
    const handlers = {
      runChecks: () => attempt(async () => {
        const response = await runConnectorChecks(connector.id)
        dispatch(checkRunLoaded(response.run))
        options.onRunStarted?.(response.run.id)
      }, { action, connectorId: connector.id }),
      sync: () => attempt(async () => {
        await syncConnector(connector.id)
        toast.success(t('actions.syncing'))
      }, { action, connectorId: connector.id }),
      toggle: () => attempt(async () => {
        const response = await updateConnector(connector.id, { isActive: !connector.isActive })
        dispatch(connectorUpserted(response.connector))
        toast.success(t('actions.saved'))
      }, { action, connectorId: connector.id }),
      delete: () => confirm({
        title: t('actions.deleteTitle'),
        // Only a connector read through a browser is ever without its account, while it waits.
        message: t(deleteBodyKeyByAccount[connector.account?.kind ?? 'browser']),
        tone: 'danger',
        confirmText: t('actions.delete'),
        onConfirm: () => attempt(async () => {
          await deleteConnector(connector.id)
          dispatch(connectorDeleted(connector.id))
          toast.success(t('actions.deleted'))
          options.onDeleted?.()
        }, { action, connectorId: connector.id }),
      }),
    } as const satisfies Record<ConnectorAction, () => unknown>
    void handlers[action]()
  }
}
