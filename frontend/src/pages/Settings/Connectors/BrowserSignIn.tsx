// Copyright © 2026 Jalapeno Labs

import type { Connector } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectBrowserSessionById } from '../../../store/browserSessionsSlice'
import { connectorUpserted } from '../../../store/connectorsSlice'
import { useAppDispatch, useAppSelector } from '../../../store/hooks'

// User interface
import { Button, Card, Chip, Spinner } from '@heroui/react'
import { BrowserStream } from '../../../browser/BrowserStream'
import { BrowserSessionActions } from './BrowserSessionActions'

// Misc
import { signInConnector } from '../../../api/routes/connectorRoutes'
import { useBrowserSessionsLoader } from '../../../hooks/useServerData'
import {
  browserStateChipColors,
  browserStateLabelKeys,
  connectorErrorMessage,
  signInChipColors,
  signInLabelKeys,
} from './connectorPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  connector: Connector
}

// A browser connector's sign-in: its own browser, streamed live to sign in on, with Start over
// and Reset as the ways out of a sign-in gone wrong. A connector waiting for its browser offers
// to start one.
export function BrowserSignIn(props: Props) {
  const { t } = useTranslation([ 'connectors', 'browsers', 'common' ])
  const dispatch = useAppDispatch()
  const status = useBrowserSessionsLoader()
  const [ isStarting, setIsStarting ] = useState(false)
  const browserId = props.connector.account?.kind === 'browser'
    ? props.connector.account.id
    : ''
  const browserSession = useAppSelector((state) => selectBrowserSessionById(state, browserId))
  const name = t(`kinds.${props.connector.kind}.name`)

  async function startBrowser() {
    setIsStarting(true)
    try {
      const response = await signInConnector(props.connector.id)
      dispatch(connectorUpserted(response.connector))
    }
    catch (error) {
      console.debug('BrowserSignIn failed to start a browser', { error, connectorId: props.connector.id })
      toast.danger(connectorErrorMessage(error, t))
    }
    finally {
      setIsStarting(false)
    }
  }

  if (!browserId) {
    return <Card className='relaxed p-6'>
      <div className='level items-start gap-4'>
        <div className='min-w-0'>
          <h2 className='font-semibold'>{t('signIn.title')}</h2>
          <p className='text-sm opacity-70'>{t('signIn.waiting', { name })}</p>
        </div>
        <Button className='shrink-0' isPending={isStarting} onPress={() => void startBrowser()}>
          <span>{t('form.startBrowser')}</span>
        </Button>
      </div>
    </Card>
  }

  if (status === 'loading' || !browserSession) {
    return <Card className='relaxed grid place-items-center p-10'>
      <Spinner />
    </Card>
  }

  const { state, error } = browserSession.status

  return <Card className='relaxed p-4'>
    <div className='compact level flex-wrap items-center gap-3'>
      <div className='flex flex-wrap items-center gap-2'>
        <h2 className='font-semibold'>{t('signIn.title')}</h2>
        <Chip size='sm' variant='soft' color={signInChipColors[props.connector.signInState]}>{
          t(signInLabelKeys[props.connector.signInState])
        }</Chip>
        <Chip size='sm' variant='soft' color={browserStateChipColors[state]}>{
          t(`browsers:${browserStateLabelKeys[state]}`)
        }</Chip>
      </div>
      <BrowserSessionActions browserSession={browserSession} name={name} />
    </div>

    {state === 'starting' && <div className='grid place-items-center gap-3 py-16'>
      <Spinner />
      <p className='text-sm opacity-70'>{t('browsers:session.starting')}</p>
    </div>}

    {state === 'failed' && <p className='py-10 text-center text-sm text-danger'>{
      error ?? t('browsers:session.failed')
    }</p>}

    {state === 'running' && <BrowserStream browserSessionId={browserSession.id} />}
  </Card>
}
