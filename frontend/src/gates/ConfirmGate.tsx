// Copyright © 2026 Jalapeno Labs

import type { KeyboardEvent, ReactNode } from 'react'

// Core
import { createContext, useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { AlertDialog, Button, Label, Switch, useOverlayState } from '@heroui/react'
import { CountdownButton } from '../components/CountdownButton'

// Misc
import { IRREVERSIBLE_CONFIRM_SECONDS } from '../constants'

// An option the action can take, shown as a switch that is off when the dialog opens, such
// as also deleting what a delete otherwise keeps.
export type ConfirmSwitch = {
  id: string
  label: string
  // Switched on, it makes the action one that can't be undone, as `isIrreversible` does.
  makesIrreversible?: boolean
}

export type ConfirmOptions = {
  title: string
  // What happens, naming what it happens to: people read the body more than the title.
  message: ReactNode
  // Defaults to "Confirm".
  confirmText?: string
  // Defaults to "Cancel", or "Got it" when the action cannot be confirmed.
  cancelText?: string
  // `danger` for destructive actions: a danger icon and button, and Enter never confirms.
  // `accent`, the default, opens with Confirm focused, so Enter confirms.
  tone?: 'danger' | 'accent'
  // For an action that can't be undone, such as a permanent delete. It is `danger`, says in
  // bold that it can't be undone, and holds Confirm for a few seconds, counting them down in
  // its label. A delete that can be undone (a soft delete) never confirms: it happens at once,
  // with Undo in its toast.
  isIrreversible?: boolean
  switches?: ConfirmSwitch[]
  // False when the action is blocked, such as deleting something still in use: the
  // dialog then only explains why, with no confirm button.
  canConfirm?: boolean
  // Receives the ids of the switches left on. The dialog stays open, its button pending,
  // until this settles. It closes when this resolves and stays open when it throws, so a
  // caller can report the failure and let the user try again or cancel.
  onConfirm: (switchedOn: ReadonlySet<string>) => Promise<void> | void
  onCancel?: () => void
}

export const ConfirmContext = createContext<(options: ConfirmOptions) => void>(() => {
  throw new Error('ConfirmGate is missing from the component tree')
})

// One question asked; the id tells each asking apart, so a new one starts its countdown over.
type Question = {
  id: number
  options: ConfirmOptions
}

type Props = {
  children: ReactNode
}

// One confirmation dialog for the whole app, opened through `useConfirm`. Callers describe
// the question and what happens on confirm, and keep no dialog state of their own.
export function ConfirmGate(props: Props) {
  const { t } = useTranslation('common')
  const overlay = useOverlayState()
  const [ question, setQuestion ] = useState<Question | null>(null)
  const [ switchedOn, setSwitchedOn ] = useState<ReadonlySet<string>>(new Set())
  const [ isPending, setIsPending ] = useState(false)

  const openOverlay = overlay.open
  const confirm = useCallback((options: ConfirmOptions) => {
    setQuestion((previous) => ({ id: (previous?.id ?? 0) + 1, options }))
    setSwitchedOn(new Set())
    openOverlay()
  }, [ openOverlay ])

  async function onConfirm() {
    if (!question) {
      console.debug('ConfirmGate confirmed with no question open')
      return
    }

    setIsPending(true)
    try {
      await question.options.onConfirm(switchedOn)
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
      question?.options.onCancel?.()
    }
    overlay.setOpen(isOpen)
  }

  function toggle(switchId: string, isSelected: boolean) {
    const next = new Set(switchedOn)
    if (isSelected) {
      next.add(switchId)
    }
    else {
      next.delete(switchId)
    }
    setSwitchedOn(next)
  }

  const options = question?.options
  const switches = options?.switches ?? []
  const canConfirm = options?.canConfirm ?? true
  const isIrreversible = canConfirm && (Boolean(options?.isIrreversible) || switches.some(
    (option) => option.makesIrreversible && switchedOn.has(option.id),
  ))
  // A blocked permanent delete still reads as one, though it has nothing to count down.
  const tone = isIrreversible || options?.isIrreversible
    ? 'danger'
    : options?.tone ?? 'accent'

  function blockEnter(event: KeyboardEvent) {
    // A destructive action is confirmed by a deliberate press, never by Enter (ux.md,
    // Hotkeys); Space on a focused Confirm still presses it.
    if (tone === 'danger' && event.key === 'Enter') {
      event.preventDefault()
      event.stopPropagation()
    }
  }

  return <ConfirmContext.Provider value={confirm}>
    {props.children}
    <AlertDialog.Backdrop
      isOpen={overlay.isOpen}
      onOpenChange={onOpenChange}
      // An alert dialog ignores Esc unless told otherwise; this one is always safe to leave.
      isKeyboardDismissDisabled={false}
    >
      <AlertDialog.Container>
        <AlertDialog.Dialog className='sm:max-w-md'>
          <AlertDialog.Header>
            <AlertDialog.Icon status={tone} />
            <AlertDialog.Heading>{options?.title}</AlertDialog.Heading>
          </AlertDialog.Header>
          <AlertDialog.Body>
            {typeof options?.message === 'string'
              ? <p className='compact'>{options.message}</p>
              : options?.message}
            {canConfirm && switches.map((option) => <Switch
              key={option.id}
              id={`confirm-${option.id}`}
              className='compact'
              isSelected={switchedOn.has(option.id)}
              isDisabled={isPending}
              onChange={(isSelected) => toggle(option.id, isSelected)}
            >
              <Switch.Content>
                <Switch.Control>
                  <Switch.Thumb />
                </Switch.Control>
                <Label>{option.label}</Label>
              </Switch.Content>
            </Switch>)}
            {isIrreversible && <p>
              <strong>{t('confirm.irreversible')}</strong>
            </p>}
          </AlertDialog.Body>
          <AlertDialog.Footer onKeyDownCapture={blockEnter}>
            <Button
              variant='tertiary'
              isDisabled={isPending}
              onPress={() => onOpenChange(false)}
            >
              <span>{
                options?.cancelText ?? (canConfirm
                  ? t('actions.cancel')
                  : t('actions.gotIt'))
              }</span>
            </Button>
            {canConfirm && <CountdownButton
              // Each question, and each time a switch makes it irreversible, counts down anew.
              key={`${question?.id}-${isIrreversible}`}
              seconds={isIrreversible
                ? IRREVERSIBLE_CONFIRM_SECONDS
                : 0}
              label={options?.confirmText ?? t('actions.confirm')}
              variant={tone === 'danger'
                ? 'danger'
                : 'primary'}
              // Focused, Enter confirms a safe action, as in any dialog (ux.md, Hotkeys).
              shouldFocus={tone === 'accent'}
              isPending={isPending}
              onPress={() => void onConfirm()}
            />}
          </AlertDialog.Footer>
        </AlertDialog.Dialog>
      </AlertDialog.Container>
    </AlertDialog.Backdrop>
  </ConfirmContext.Provider>
}
