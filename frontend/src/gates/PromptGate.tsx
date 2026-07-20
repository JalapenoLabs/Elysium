// Copyright © 2026 Jalapeno Labs

import type { FormEvent, ReactNode } from 'react'
import type { ButtonProps, InputProps } from '@heroui/react'

// Core
import { createContext, useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Button,
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
} from '@heroui/react'

// A polished, app-wide single-value prompt used instead of the native
// `window.prompt`. Open it from anywhere via `usePrompt()`; the value is
// optionally validated with a Zod schema before confirm is allowed.

export type PromptOptions = {
  // Pre-translated title and (optional) message.
  title: ReactNode
  message?: ReactNode
  defaultValue?: string
  // Extra props forwarded to the input (label, placeholder, type, ...).
  inputProps?: Partial<Omit<InputProps, 'value' | 'onValueChange'>>
  // Rejects invalid input before confirm: return an error message to block, or
  // null to allow. The error shows under the field.
  validate?: (value: string) => string | null
  confirmLabel?: string
  cancelLabel?: string
  confirmColor?: ButtonProps['color']
  onConfirm: (value: string) => void | Promise<void>
  onCancel?: () => void
}

export const PromptContext = createContext<(options: PromptOptions) => void>(() => {
  throw new Error('PromptGate is missing from the tree')
})

type Props = {
  children: ReactNode
}

export function PromptGate(props: Props) {
  const { t } = useTranslation('common')
  const [ options, setOptions ] = useState<PromptOptions | null>(null)
  const [ value, setValue ] = useState('')
  const [ isBusy, setIsBusy ] = useState(false)

  const openPrompt = useCallback((next: PromptOptions) => {
    setOptions(next)
    setValue(next.defaultValue ?? '')
  }, [])

  function onCancel() {
    options?.onCancel?.()
    setOptions(null)
  }

  // Validate on every render so the confirm button and error track the field.
  const validationError = options?.validate ? options.validate(value) : null
  const errorMessage = value ? validationError : null
  const canConfirm = !validationError

  async function onConfirm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!options || !canConfirm) {
      return
    }
    setIsBusy(true)
    try {
      await options.onConfirm(value)
      setOptions(null)
    }
    catch (error) {
      console.debug('prompt action failed', { error })
    }
    finally {
      setIsBusy(false)
    }
  }

  return <PromptContext.Provider value={openPrompt}>
    { props.children }
    <Modal isOpen={Boolean(options)} onClose={onCancel} isDismissable={!isBusy}>
      <ModalContent>
        <form onSubmit={onConfirm}>
          <ModalHeader>{ options?.title }</ModalHeader>
          <ModalBody>
            { options?.message
              ? <p className='opacity-80'>{ options.message }</p>
              : null }
            <Input
              autoFocus
              size='sm'
              radius='sm'
              isDisabled={isBusy}
              { ...options?.inputProps }
              value={value}
              onValueChange={setValue}
              isInvalid={Boolean(errorMessage)}
              errorMessage={errorMessage ?? undefined}
            />
          </ModalBody>
          <ModalFooter>
            <Button size='sm' radius='sm' variant='light' isDisabled={isBusy} onPress={onCancel}>
              <span>{ options?.cancelLabel ?? t('actions.cancel') }</span>
            </Button>
            <Button
              type='submit'
              size='sm'
              radius='sm'
              color={options?.confirmColor ?? 'primary'}
              isLoading={isBusy}
              isDisabled={!canConfirm}
            >
              <span>{ options?.confirmLabel ?? t('actions.confirm') }</span>
            </Button>
          </ModalFooter>
        </form>
      </ModalContent>
    </Modal>
  </PromptContext.Provider>
}
