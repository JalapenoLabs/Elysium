// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { createContext, useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { AlertDialog, Button, useOverlayState } from '@heroui/react'

export type ConfirmOptions = {
  title: string
  message: ReactNode
  // Defaults to "Confirm".
  confirmText?: string
  // Defaults to "Cancel", or "Close" when the action cannot be confirmed.
  cancelText?: string
  // `danger` for destructive actions: a danger icon and a danger confirm button.
  tone?: 'danger' | 'accent'
  // False when the action is blocked, such as deleting something still in use: the
  // dialog then only explains why, with no confirm button.
  canConfirm?: boolean
  // The dialog stays open, its button pending, until this settles. It closes when this
  // resolves and stays open when it throws, so a caller can report the failure and let
  // the user try again or cancel.
  onConfirm: () => Promise<void> | void
  onCancel?: () => void
}

export const ConfirmContext = createContext<(options: ConfirmOptions) => void>(() => {
  throw new Error('ConfirmGate is missing from the component tree')
})

type Props = {
  children: ReactNode
}

// One confirmation dialog for the whole app, opened through `useConfirm`. Callers describe
// the question and what happens on confirm, and keep no dialog state of their own.
export function ConfirmGate(props: Props) {
  const { t } = useTranslation('common')
  const overlay = useOverlayState()
  const [ options, setOptions ] = useState<ConfirmOptions | null>(null)
  const [ isPending, setIsPending ] = useState(false)

  const openOverlay = overlay.open
  const confirm = useCallback((nextOptions: ConfirmOptions) => {
    setOptions(nextOptions)
    openOverlay()
  }, [ openOverlay ])

  async function onConfirm() {
    if (!options) {
      console.debug('ConfirmGate confirmed with no question open')
      return
    }

    setIsPending(true)
    try {
      await options.onConfirm()
      overlay.close()
    }
    catch (error) {
      console.debug('ConfirmGate kept the dialog open: the confirmed action failed', { error })
    }
    finally {
      setIsPending(false)
    }
  }

  function onOpenChange(isOpen: boolean) {
    // Dismissing while the action runs would hide its outcome.
    if (isPending) {
      return
    }
    if (!isOpen) {
      options?.onCancel?.()
    }
    overlay.setOpen(isOpen)
  }

  const tone = options?.tone ?? 'accent'
  const canConfirm = options?.canConfirm ?? true

  return <ConfirmContext.Provider value={confirm}>
    {props.children}
    <AlertDialog.Backdrop isOpen={overlay.isOpen} onOpenChange={onOpenChange}>
      <AlertDialog.Container>
        <AlertDialog.Dialog className='sm:max-w-md'>
          <AlertDialog.Header>
            <AlertDialog.Icon status={tone} />
            <AlertDialog.Heading>{options?.title}</AlertDialog.Heading>
          </AlertDialog.Header>
          <AlertDialog.Body>{
            typeof options?.message === 'string'
              ? <p>{options.message}</p>
              : options?.message
          }</AlertDialog.Body>
          <AlertDialog.Footer>
            <Button
              variant='tertiary'
              isDisabled={isPending}
              onPress={() => onOpenChange(false)}
            >
              <span>{
                options?.cancelText ?? (canConfirm
                  ? t('actions.cancel')
                  : t('actions.close'))
              }</span>
            </Button>
            {canConfirm && <Button
              variant={tone === 'danger'
                ? 'danger'
                : 'primary'}
              isPending={isPending}
              onPress={onConfirm}
            >
              <span>{options?.confirmText ?? t('actions.confirm')}</span>
            </Button>}
          </AlertDialog.Footer>
        </AlertDialog.Dialog>
      </AlertDialog.Container>
    </AlertDialog.Backdrop>
  </ConfirmContext.Provider>
}
