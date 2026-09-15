// Copyright © 2026 Jalapeno Labs

import type { FormEvent, ReactNode } from 'react'

// Core
import { createContext, useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Form, Input, Label, Modal, TextArea, TextField, useOverlayState } from '@heroui/react'

export type PromptOptions = {
  title: string
  message?: ReactNode
  // The field's label.
  label: string
  // The field's starting value, such as the current name when renaming.
  defaultValue?: string
  placeholder?: string
  // A hint under the field.
  description?: string
  // Defaults to "Save".
  submitText?: string
  // Whether an empty answer may be submitted.
  isOptional?: boolean
  maxLength?: number
  // A multi-line text area instead of a single line.
  isMultiline?: boolean
  // Receives the trimmed answer. The dialog stays open, its button pending, until this
  // settles; it closes when this resolves and stays open when it throws.
  onSubmit: (value: string) => Promise<void> | void
  onCancel?: () => void
}

export const PromptContext = createContext<(options: PromptOptions) => void>(() => {
  throw new Error('PromptGate is missing from the component tree')
})

type Props = {
  children: ReactNode
}

// One single-field dialog for the whole app, opened through `usePrompt`.
export function PromptGate(props: Props) {
  const { t } = useTranslation('common')
  const overlay = useOverlayState()
  const [ options, setOptions ] = useState<PromptOptions | null>(null)
  const [ value, setValue ] = useState('')
  const [ isPending, setIsPending ] = useState(false)

  const openOverlay = overlay.open
  const prompt = useCallback((nextOptions: PromptOptions) => {
    setOptions(nextOptions)
    setValue(nextOptions.defaultValue ?? '')
    openOverlay()
  }, [ openOverlay ])

  const trimmed = value.trim()
  const isValid = Boolean(options?.isOptional || trimmed)

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!options || !isValid) {
      console.debug('PromptGate submitted without an open prompt or a required answer')
      return
    }

    setIsPending(true)
    try {
      await options.onSubmit(trimmed)
      overlay.close()
    }
    catch (error) {
      console.debug('PromptGate kept the dialog open: the submitted action failed', { error })
    }
    finally {
      setIsPending(false)
    }
  }

  function onOpenChange(isOpen: boolean) {
    if (isPending) {
      return
    }
    if (!isOpen) {
      options?.onCancel?.()
    }
    overlay.setOpen(isOpen)
  }

  return <PromptContext.Provider value={prompt}>
    {props.children}
    <Modal.Backdrop isOpen={overlay.isOpen} onOpenChange={onOpenChange}>
      <Modal.Container>
        <Modal.Dialog className='sm:max-w-md'>
          <Modal.CloseTrigger />
          <Modal.Header>
            <Modal.Heading>{options?.title}</Modal.Heading>
          </Modal.Header>
          <Form onSubmit={onSubmit} validationBehavior='aria'>
            <Modal.Body className='mt-2 flex flex-col gap-3'>
              {typeof options?.message === 'string'
                ? <p className='text-sm opacity-80'>{options.message}</p>
                : options?.message}
              <TextField
                autoFocus
                isRequired={!options?.isOptional}
                maxLength={options?.maxLength}
                value={value}
                onChange={setValue}
              >
                <Label>{options?.label}</Label>
                {options?.isMultiline
                  ? <TextArea rows={3} placeholder={options.placeholder} />
                  : <Input placeholder={options?.placeholder} />}
                {options?.description && <Description>{options.description}</Description>}
              </TextField>
            </Modal.Body>
            <Modal.Footer>
              <Button
                variant='tertiary'
                isDisabled={isPending}
                onPress={() => onOpenChange(false)}
              >
                <span>{t('actions.cancel')}</span>
              </Button>
              <Button
                type='submit'
                isDisabled={!isValid}
                isPending={isPending}
              >
                <span>{options?.submitText ?? t('actions.save')}</span>
              </Button>
            </Modal.Footer>
          </Form>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  </PromptContext.Provider>
}
