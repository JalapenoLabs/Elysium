// Copyright © 2026 Jalapeno Labs

import type { BrowserSession } from '../../../api/routes/browserSessionRoutes'
import type { ConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import {
  connectorAccountDeleted,
  connectorAccountUpserted,
  selectAllConnectorAccounts,
} from '../../../store/connectorAccountsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'

// User interface
import { Button, Card, Chip, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { createConnectorAccount, deleteConnectorAccount } from '../../../api/routes/connectorRoutes'
import { useConnectorAccountsLoader } from '../../../hooks/useServerData'
import {
  connectorKindFor,
  connectorLabelKeys,
  signInChipColors,
  signInLabelKeys,
} from './connectorPresentation'

type Props = {
  browserSession: BrowserSession
}

// Whether Elysium reads this browser's mailbox, and the switch to start or stop. Only shown
// for a browser started on a mailbox a connector can read.
export function MailReadingSection(props: Props) {
  const { t } = useTranslation([ 'browsers', 'common' ])
  const dispatch = useAppDispatch()
  const status = useConnectorAccountsLoader()
  const accounts = useAppSelector(selectAllConnectorAccounts)
  const [ isBusy, setIsBusy ] = useState(false)

  const kind = connectorKindFor(props.browserSession.startUrl)
  const account = accounts.find((candidate) => candidate.browserSessionId === props.browserSession.id)
  if (!kind || status !== 'loaded') {
    return null
  }

  async function startReading(connectorKind: ConnectorKind) {
    setIsBusy(true)
    try {
      const response = await createConnectorAccount({
        kind: connectorKind,
        browserSessionId: props.browserSession.id,
      })
      dispatch(connectorAccountUpserted(response.connector))
      toast.success(t('connectors.toasts.started'))
    }
    catch (error) {
      console.debug('MailReadingSection failed to start reading', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsBusy(false)
    }
  }

  async function stopReading(accountId: string) {
    setIsBusy(true)
    try {
      await deleteConnectorAccount(accountId)
      dispatch(connectorAccountDeleted(accountId))
      toast.success(t('connectors.toasts.stopped'))
    }
    catch (error) {
      console.debug('MailReadingSection failed to stop reading', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsBusy(false)
    }
  }

  return <Card className='relaxed p-4'>
    <div className='level items-start gap-4'>
      <div className='min-w-0'>
        <div className='flex items-center gap-2'>
          <h2 className='text-base font-semibold'>{
            t('connectors.title', { service: t(connectorLabelKeys[kind]) })
          }</h2>
          {account && <Chip size='sm' variant='soft' color={signInChipColors[account.signInState]}>{
            t(signInLabelKeys[account.signInState])
          }</Chip>}
        </div>
        <p className='text-sm opacity-70'>{
          account
            ? t('connectors.reading')
            : t('connectors.offer')
        }</p>
        {account?.lastError && <p className='mt-1 text-sm text-danger'>{account.lastError}</p>}
      </div>
      {account
        ? <Button size='sm' variant='outline' isPending={isBusy} onPress={() => stopReading(account.id)}>
          <span>{t('connectors.stop')}</span>
        </Button>
        : <Button size='sm' isPending={isBusy} onPress={() => startReading(kind)}>
          <span>{t('connectors.start')}</span>
        </Button>}
    </div>
  </Card>
}
