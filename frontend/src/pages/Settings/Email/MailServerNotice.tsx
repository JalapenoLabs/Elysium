// Copyright © 2026 Jalapeno Labs

import type { MailServer } from '../../../api/routes/mailRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Alert, Button } from '@heroui/react'
import { LuServer } from 'react-icons/lu'

type Props = {
  server: MailServer
  onSetUp: () => void
}

// Says why self-hosted mailboxes are unavailable, when they are: the bundled server waits
// for its setup, or it is not answering. Renders nothing once the server is ready.
export function MailServerNotice(props: Props) {
  const { t } = useTranslation('email')

  if (props.server.status === 'ready') {
    return null
  }

  // Setting up again cannot help a server that is down, so this offers no action.
  if (props.server.status === 'unreachable') {
    return <Alert status='warning' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Title>{
          t('availability.mailServerUnreachable.title')
        }</Alert.Title>
        <Alert.Description>{
          t('availability.mailServerUnreachable.description', { error: props.server.error ?? '' })
        }</Alert.Description>
      </Alert.Content>
    </Alert>
  }

  return <Alert status='accent' className='compact'>
    <Alert.Indicator>
      <LuServer className='size-4' aria-hidden />
    </Alert.Indicator>
    <Alert.Content>
      <Alert.Title>{
        t('mailServer.title')
      }</Alert.Title>
      <Alert.Description>{
        t('mailServer.description')
      }</Alert.Description>
      {props.server.error && <Alert.Description className='mt-1 block'>{
        t('mailServer.setUpAgain', { error: props.server.error })
      }</Alert.Description>}
      <Button size='sm' className='mt-3' onPress={props.onSetUp}>
        <span>{t('mailServer.action')}</span>
      </Button>
    </Alert.Content>
  </Alert>
}
