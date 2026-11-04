// Copyright © 2026 Jalapeno Labs

import type { ConnectorKindDefinition } from '../../../api/routes/connectorRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { connectorUpserted } from '../../../store/connectorsSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Card } from '@heroui/react'
import { LuAppWindow } from 'react-icons/lu'
import { HelpPopover } from '../../../components/HelpPopover'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'

// Misc
import { createConnector } from '../../../api/routes/connectorRoutes'
import { getConnectorUrl } from '../../../urls'
import { usePageSave } from '../../../layout/page/pageSave'
import { connectorErrorMessage, startingContent } from './connectorPresentation'
import { toast } from '../../../toast/toast'

type Props = {
  definition: ConnectorKindDefinition
}

// A kind read through a browser starts here: the connector is made with its own browser, and
// its page shows the browser's stream to sign in on, then what it reads and its checks. It
// starts reading everything it can, narrowed there once its person has signed in. Nothing is
// typed first, so the page's Save, Start browser, can be pressed at once, and leaving never asks.
export function NewBrowserConnector(props: Props) {
  const { t } = useTranslation([ 'connectors', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const name = t(`kinds.${props.definition.kind}.name`)

  async function start() {
    try {
      const response = await createConnector({
        kind: props.definition.kind,
        account: { new: {}},
        content: startingContent(props.definition),
      })
      dispatch(connectorUpserted(response.connector))
      void navigate(getConnectorUrl(response.connector.id))
      return true
    }
    catch (error) {
      console.debug('NewBrowserConnector failed to start its browser', { error, kind: props.definition.kind })
      toast.danger(connectorErrorMessage(error, t))
      return false
    }
  }

  usePageSave({
    save: start,
    isDirty: false,
    canSaveUnchanged: true,
    label: t('form.startBrowser'),
  })

  return <>
    <Card className='relaxed p-6'>
      <div className='flex min-w-0 gap-3'>
        <LuAppWindow className='mt-0.5 size-5 shrink-0 text-accent' aria-hidden />
        <div className='flex min-w-0 items-center gap-1'>
          <h2 className='font-semibold'>{t('form.signInTo', { name })}</h2>
          <HelpPopover>{t('form.browserSignIn', { name })}</HelpPopover>
        </div>
      </div>
    </Card>
    <PageCreateFooter />
  </>
}
