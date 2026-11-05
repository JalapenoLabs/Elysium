// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Form, Input, Label, Modal, TextField } from '@heroui/react'

// Utility
import { defaultUrlTransform } from 'react-markdown'

// Misc
import { useInitialFocus } from '../../hooks/useInitialFocus'

type Props = {
  onInsert: (address: string, description: string) => void
  onClose: () => void
}

// Adds an image by its address, with a description for whoever cannot see it.
export function EditorImageDialog(props: Props) {
  const { t } = useTranslation('common')
  const [ address, setAddress ] = useState('')
  const [ description, setDescription ] = useState('')
  const fieldRef = useInitialFocus()
  const trimmed = address.trim()
  const isValid = Boolean(trimmed && defaultUrlTransform(trimmed))

  return <Modal.Backdrop
    isOpen
    onOpenChange={(isOpen) => {
      if (!isOpen) {
        props.onClose()
      }
    }}
  >
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('markdownEditor.image.title')}</Modal.Heading>
        </Modal.Header>
        <Form
          validationBehavior='aria'
          onSubmit={(event) => {
            event.preventDefault()
            if (isValid) {
              props.onInsert(trimmed, description.trim())
            }
          }}
        >
          <Modal.Body className='mt-2 flex flex-col gap-3'>
            <TextField id='markdown-editor-image-address' value={address} onChange={setAddress}>
              <Label>{t('markdownEditor.image.address')}</Label>
              <Input ref={fieldRef} autoComplete='off' placeholder='https://' />
            </TextField>
            <TextField id='markdown-editor-image-description' value={description} onChange={setDescription}>
              <Label>{t('markdownEditor.image.description')}</Label>
              <Input autoComplete='off' />
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('actions.cancel')}</span>
            </Button>
            <Button type='submit' isDisabled={!isValid}>
              <span>{t('markdownEditor.image.insert')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
