// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { ButtonProps } from '@heroui/react'

// Core
import { createContext, useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Button,
  Modal,
  ModalBody,
  ModalContent,
  ModalFooter,
  ModalHeader,
} from '@heroui/react'

// A polished, app-wide confirmation modal used instead of the native
// `window.confirm`, so every "are you sure?" reads consistently and can run an
// async action with a loading state. Open it from anywhere via `useConfirm()`.

export type ConfirmOptions = {
  // Pre-translated title and message (a string or any node).
  title: ReactNode
  message: ReactNode
  // Button labels; default to the shared "Confirm" / "Cancel".
  confirmLabel?: string
  cancelLabel?: string
  // The confirm button's color; use 'danger' for destructive actions.
  confirmColor?: ButtonProps['color']
  // When true, shows only the dismiss button (an acknowledgement, not a choice).
  dismissOnly?: boolean
  // Runs when confirmed. The modal shows a loading state until it settles, then
  // closes; a throw leaves the modal open so the caller can surface the error.
  onConfirm?: () => void | Promise<void>
  onCancel?: () => void
}

// Defaults to a thrower so a stray `useConfirm()` outside the provider is loud.
export const ConfirmContext = createContext<(options: ConfirmOptions) => void>(() => {
  throw new Error('ConfirmGate is missing from the tree')
})

type Props = {
  children: ReactNode
}

export function ConfirmGate(props: Props) {
  const { t } = useTranslation('common')
  const [ options, setOptions ] = useState<ConfirmOptions | null>(null)
  const [ isBusy, setIsBusy ] = useState(false)

  const openConfirm = useCallback((next: ConfirmOptions) => {
    setOptions(next)
  }, [])

  function onCancel() {
    options?.onCancel?.()
    setOptions(null)
  }

  async function onConfirm() {
    if (!options) {
      return
    }
    setIsBusy(true)
    try {
      await options.onConfirm?.()
      setOptions(null)
    }
    catch (error) {
      // Keep the modal open so the caller's own error handling can show why.
      console.debug('confirm action failed', { error })
    }
    finally {
      setIsBusy(false)
    }
  }

  // Enter confirms while the modal is open (Escape is handled by the modal). A
  // dismiss-only modal has nothing to confirm, so Enter just dismisses.
  useEffect(() => {
    if (!options) {
      return undefined
    }
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === 'Enter' && !isBusy) {
        event.preventDefault()
        if (options?.dismissOnly) {
          onCancel()
        }
        else {
          void onConfirm()
        }
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [ options, isBusy ])

  return <ConfirmContext.Provider value={openConfirm}>
    { props.children }
    <Modal
      isOpen={Boolean(options)}
      onClose={onCancel}
      isDismissable={!isBusy}
      isKeyboardDismissDisabled={isBusy}
    >
      <ModalContent>
        <ModalHeader>{ options?.title }</ModalHeader>
        <ModalBody>
          { typeof options?.message === 'string'
            ? <p className='opacity-80'>{ options.message }</p>
            : options?.message }
        </ModalBody>
        <ModalFooter>
          <Button size='sm' radius='sm' variant='light' isDisabled={isBusy} onPress={onCancel}>
            <span>{ options?.cancelLabel ?? t('actions.cancel') }</span>
          </Button>
          { options?.dismissOnly
            ? null
            : <Button
              size='sm'
              radius='sm'
              color={options?.confirmColor ?? 'primary'}
              isLoading={isBusy}
              onPress={onConfirm}
            >
              <span>{ options?.confirmLabel ?? t('actions.confirm') }</span>
            </Button> }
        </ModalFooter>
      </ModalContent>
    </Modal>
  </ConfirmContext.Provider>
}
