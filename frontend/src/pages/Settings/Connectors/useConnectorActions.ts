// Copyright © 2026 Jalapeno Labs

import type { Connector } from '../../../api/routes/connectorRoutes'
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

// The kinds whose connector goes with its account.
const REMOVES_ACCOUNT = new Set([ 'jira', 'github' ])

// What the hub's gear menu does, shared by the list and a connector's page. Running checks
// answers the run, which the caller follows.
export function useConnectorActions(onRunStarted?: (runId: string) => void) {
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
        onRunStarted?.(response.run.id)
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
        message: REMOVES_ACCOUNT.has(connector.kind)
          ? t('actions.deleteBodyWithAccount', { kind: t(`kinds.${connector.kind}.name`) })
          : t('actions.deleteBody'),
        tone: 'danger',
        confirmText: t('actions.delete'),
        onConfirm: () => attempt(async () => {
          await deleteConnector(connector.id)
          dispatch(connectorDeleted(connector.id))
          toast.success(t('actions.deleted'))
        }, { action, connectorId: connector.id }),
      }),
    } as const satisfies Record<ConnectorAction, () => unknown>
    void handlers[action]()
  }
}
