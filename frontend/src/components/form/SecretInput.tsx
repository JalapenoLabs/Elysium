// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Input, Tooltip } from '@heroui/react'
import { FiEye, FiEyeOff } from 'react-icons/fi'

// A masked credential field with a reveal toggle, for connection forms (S3
// secrets, SFTP passwords, API tokens). Masked by default so a shared screen
// never leaks a secret; the toggle is per-field and never persisted.
type Props = {
  label: string
  value: string
  onValueChange: (value: string) => void
  isDisabled?: boolean
  isInvalid?: boolean
  errorMessage?: string
  className?: string
}

export function SecretInput(props: Props) {
  const { t } = useTranslation()
  const [ isRevealed, setIsRevealed ] = useState(false)

  const toggleLabel = isRevealed
    ? t('secret.hide')
    : t('secret.reveal')

  return <Input
    label={props.label}
    type={isRevealed ? 'text' : 'password'}
    value={props.value}
    onValueChange={props.onValueChange}
    isDisabled={props.isDisabled}
    isInvalid={props.isInvalid}
    errorMessage={props.errorMessage}
    className={props.className}
    autoComplete='off'
    endContent={
      <Tooltip content={toggleLabel}>
        <div>
          <Button
            isIconOnly
            size='sm'
            variant='light'
            aria-label={toggleLabel}
            onPress={() => setIsRevealed((wasRevealed) => !wasRevealed)}
          >
            {isRevealed
              ? <FiEyeOff className='h-4 w-4 opacity-60' />
              : <FiEye className='h-4 w-4 opacity-60' />}
          </Button>
        </div>
      </Tooltip>
    }
  />
}
