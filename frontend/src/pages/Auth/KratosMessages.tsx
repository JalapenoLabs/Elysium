// Copyright © 2026 Jalapeno Labs

import type { UiText } from '@ory/client-fetch'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Alert } from '@heroui/react'

// Misc
import { describeKratosMessage, kratosMessageStatus } from './kratosPresentation'

type Props = {
  messages: UiText[]
}

// Messages Kratos sent for a whole form, such as "wrong email or password".
export function KratosMessages(props: Props) {
  const { t } = useTranslation('auth')

  if (!props.messages.length) {
    return null
  }

  return <div className='compact flex flex-col gap-2'>{
    props.messages.map((message) => {
      const described = describeKratosMessage(message)
      return <Alert key={`${message.id}-${message.text}`} status={kratosMessageStatus(message)}>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Description>{
            described.key
              ? t(described.key)
              : described.text
          }</Alert.Description>
        </Alert.Content>
      </Alert>
    })
  }</div>
}
