// Copyright © 2026 Jalapeno Labs

import type { ButtonProps } from '@heroui/react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button } from '@heroui/react'

type Props = {
  // How long the button stays disabled once it mounts, its label counting the seconds down.
  // Zero enables it at once. Give it a new `key` to start the count again.
  seconds: number
  label: string
  variant?: ButtonProps['variant']
  autoFocus?: boolean
  isDisabled?: boolean
  isPending?: boolean
  onPress: () => void
}

// A button that can't be pressed until its seconds have passed, so a dialog confirming
// something that can't be undone is read before it is confirmed.
export function CountdownButton(props: Props) {
  const { t } = useTranslation('common')
  const [ secondsLeft, setSecondsLeft ] = useState(props.seconds)

  useEffect(() => {
    if (secondsLeft <= 0) {
      return undefined
    }
    const timer = window.setTimeout(() => setSecondsLeft(secondsLeft - 1), 1000)
    return () => window.clearTimeout(timer)
  }, [ secondsLeft ])

  return <Button
    variant={props.variant}
    autoFocus={props.autoFocus}
    isDisabled={secondsLeft > 0 || props.isDisabled}
    isPending={props.isPending}
    onPress={props.onPress}
  >
    <span>{
      secondsLeft > 0
        ? t('confirm.countdown', { action: props.label, seconds: secondsLeft })
        : props.label
    }</span>
  </Button>
}
