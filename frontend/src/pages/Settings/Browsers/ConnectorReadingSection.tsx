// Copyright © 2026 Jalapeno Labs

import type { BrowserSession } from '../../../api/routes/browserSessionRoutes'
import type { BrowserConnectorKind } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import {
  connectorDeleted,
  connectorUpserted,
  selectAllConnectors,
} from '../../../store/connectorsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'

// User interface
import { Button, Card, Chip, toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { createConnector, deleteConnector } from '../../../api/routes/connectorRoutes'
import { useConnectorsLoader } from '../../../hooks/useServerData'
import {
  connectorKindFor,
  connectorLabelKeys,
  readingCopyByKind,
  signInChipColors,
  signInLabelKeys,
  startingContentByKind,
} from './connectorPresentation'

type Props = {
  browserSession: BrowserSession
}

// Whether Elysium reads what this browser is signed in to (a mailbox, or Slack), and the switch
// to start or stop. Only shown for a browser started on a page a connector can read.
export function ConnectorReadingSection(props: Props) {
  const { t } = useTranslation([ 'browsers', 'common' ])
  const dispatch = useAppDispatch()
  const status = useConnectorsLoader()
  const connectors = useAppSelector(selectAllConnectors)
  const [ isBusy, setIsBusy ] = useState(false)

  const kind = connectorKindFor(props.browserSession.startUrl)
  // One browser can feed several kinds (a Microsoft sign-in feeds Outlook and Teams), so the
  // connector shown here is this page's kind.
  const account = connectors.find((candidate) => candidate.kind === kind
    && candidate.account?.kind === 'browser'
    && candidate.account.id === props.browserSession.id)
  if (!kind || status !== 'loaded') {
    return null
  }

  const copy = readingCopyByKind[kind]

  async function startReading(connectorKind: BrowserConnectorKind) {
    setIsBusy(true)
    try {
      const response = await createConnector({
        kind: connectorKind,
        account: { id: props.browserSession.id },
        content: { ...startingContentByKind[connectorKind] },
      })
      dispatch(connectorUpserted(response.connector))
      toast.success(t(copy.started))
    }
    catch (error) {
      console.debug('ConnectorReadingSection failed to start reading', { error })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsBusy(false)
    }
  }

  async function stopReading(accountId: string) {
    setIsBusy(true)
    try {
      await deleteConnector(accountId)
      dispatch(connectorDeleted(accountId))
      toast.success(t(copy.stopped))
    }
    catch (error) {
      console.debug('ConnectorReadingSection failed to stop reading', { error })
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
            t(copy.title, { service: t(connectorLabelKeys[kind]) })
          }</h2>
          {account && <Chip size='sm' variant='soft' color={signInChipColors[account.signInState]}>{
            t(signInLabelKeys[account.signInState])
          }</Chip>}
        </div>
        <p className='text-sm opacity-70'>{
          account
            ? t(copy.reading)
            : t(copy.offer)
        }</p>
        {account?.lastError && <p className='mt-1 text-sm text-danger'>{account.lastError}</p>}
      </div>
      {account
        ? <Button size='sm' variant='outline' isPending={isBusy} onPress={() => stopReading(account.id)}>
          <span>{t('connectors.stop')}</span>
        </Button>
        : <Button size='sm' isPending={isBusy} onPress={() => startReading(kind)}>
          <span>{t(copy.start)}</span>
        </Button>}
    </div>
  </Card>
}
