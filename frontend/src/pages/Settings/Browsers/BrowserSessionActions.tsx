// Copyright © 2026 Jalapeno Labs

import type { BrowserSession } from '../../../api/routes/browserSessionRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { browserSessionUpserted } from '../../../store/browserSessionsSlice'
import { useAppDispatch } from '../../../store/hooks'

// User interface
import { Button, Tooltip, toast } from '@heroui/react'
import { LuRotateCcw, LuTrash2 } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { resetBrowserSession, startOverBrowserSession } from '../../../api/routes/browserSessionRoutes'
import { useConfirm } from '../../../hooks/useConfirm'

type Props = {
  browserSession: BrowserSession
}

// The ways out of a browser stuck somewhere: back to its sign-in page, still signed in; or an
// empty profile, signed out of everything.
export function BrowserSessionActions(props: Props) {
  const { t } = useTranslation([ 'browsers', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const [ isStartingOver, setIsStartingOver ] = useState(false)
  const { state } = props.browserSession.status

  async function startOver() {
    setIsStartingOver(true)
    try {
      await startOverBrowserSession(props.browserSession.id)
      toast.success(t('toasts.startedOver', { name: props.browserSession.name }))
    }
    catch (error) {
      console.debug('BrowserSessionActions failed to start a browser over', { error, id: props.browserSession.id })
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
    finally {
      setIsStartingOver(false)
    }
  }

  function confirmReset() {
    confirm({
      title: t('reset.title', { name: props.browserSession.name }),
      message: t('reset.body'),
      tone: 'danger',
      confirmText: t('reset.confirm'),
      onConfirm: async () => {
        try {
          const response = await resetBrowserSession(props.browserSession.id)
          dispatch(browserSessionUpserted(response.browserSession))
        }
        catch (error) {
          console.debug('BrowserSessionActions failed to reset a browser', { error, id: props.browserSession.id })
          toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }
        toast.success(t('toasts.reset', { name: props.browserSession.name }))
      },
    })
  }

  return <div className='flex flex-wrap gap-2'>
    <Tooltip delay={400}>
      <Tooltip.Trigger>
        <Button
          size='sm'
          variant='outline'
          isDisabled={state !== 'running'}
          isPending={isStartingOver}
          onPress={() => void startOver()}
        >
          <LuRotateCcw aria-hidden />
          <span>{t('session.startOver')}</span>
        </Button>
      </Tooltip.Trigger>
      <Tooltip.Content placement='bottom' className='max-w-xs break-normal'>{
        t('session.startOverHint')
      }</Tooltip.Content>
    </Tooltip>
    <Tooltip delay={400}>
      <Tooltip.Trigger>
        <Button
          size='sm'
          variant='danger-soft'
          isDisabled={state === 'starting'}
          onPress={confirmReset}
        >
          <LuTrash2 aria-hidden />
          <span>{t('session.reset')}</span>
        </Button>
      </Tooltip.Trigger>
      <Tooltip.Content placement='bottom' className='max-w-xs break-normal'>{
        t('session.resetHint')
      }</Tooltip.Content>
    </Tooltip>
  </div>
}
