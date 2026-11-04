// Copyright © 2026 Jalapeno Labs

import type { ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { connectorUpserted } from '../../../store/connectorsSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Button, Card } from '@heroui/react'
import { LuAppWindow } from 'react-icons/lu'

// Misc
import { createConnector } from '../../../api/routes/connectorRoutes'
import { getConnectorUrl } from '../../../urls'
import { connectorErrorMessage, startingContent } from './connectorPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  definition: ConnectorKindDefinition
}

// A kind read through a browser starts here: the connector is made with its own browser, and
// its page shows the browser's stream to sign in on, then what it reads and its checks. It
// starts reading everything it can, narrowed there once its person has signed in.
export function NewBrowserConnector(props: Props) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const [ isStarting, setIsStarting ] = useState(false)
  const name = t(`kinds.${props.definition.kind}.name`)

  async function start() {
    setIsStarting(true)
    try {
      const response = await createConnector({
        kind: props.definition.kind,
        account: { new: {}},
        content: startingContent(props.definition),
      })
      dispatch(connectorUpserted(response.connector))
      void navigate(getConnectorUrl(response.connector.id))
    }
    catch (error) {
      console.debug('NewBrowserConnector failed to start its browser', { error, kind: props.definition.kind })
      toast.danger(connectorErrorMessage(error, t))
      setIsStarting(false)
    }
  }

  return <Card className='relaxed p-6'>
    <div className='level items-start gap-4'>
      <div className='flex min-w-0 gap-3'>
        <LuAppWindow className='mt-0.5 size-5 shrink-0 text-accent' aria-hidden />
        <div className='min-w-0'>
          <h2 className='font-semibold'>{t('form.signIn')}</h2>
          <p className='text-sm opacity-70'>{t('form.browserSignIn', { name })}</p>
        </div>
      </div>
      <Button className='shrink-0' isPending={isStarting} onPress={() => void start()}>
        <span>{t('form.startBrowser')}</span>
      </Button>
    </div>
  </Card>
}
