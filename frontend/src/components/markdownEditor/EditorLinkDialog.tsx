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
  // What the selection links to, if anything.
  address: string
  onApply: (address: string) => void
  onRemove: () => void
  onClose: () => void
}

// Sets the address of the selected text. Only an address the page would keep can be applied.
export function EditorLinkDialog(props: Props) {
  const { t } = useTranslation('common')
  const [ address, setAddress ] = useState(props.address)
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
          <Modal.Heading>{t('markdownEditor.link.title')}</Modal.Heading>
        </Modal.Header>
        <Form
          validationBehavior='aria'
          onSubmit={(event) => {
            event.preventDefault()
            if (isValid) {
              props.onApply(trimmed)
            }
          }}
        >
          <Modal.Body className='mt-2'>
            <TextField id='markdown-editor-link-address' value={address} onChange={setAddress}>
              <Label>{t('markdownEditor.link.address')}</Label>
              <Input ref={fieldRef} autoComplete='off' placeholder='https://' />
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            {props.address && <Button variant='danger-soft' className='sm:mr-auto' onPress={props.onRemove}>
              <span>{t('markdownEditor.link.remove')}</span>
            </Button>}
            <Button slot='close' variant='tertiary'>
              <span>{t('actions.cancel')}</span>
            </Button>
            <Button type='submit' isDisabled={!isValid}>
              <span>{t('markdownEditor.link.apply')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
