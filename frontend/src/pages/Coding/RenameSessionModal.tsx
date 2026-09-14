// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { CodingSession } from '../../api/routes/codingSessionRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { codingSessionUpserted } from '../../store/codingSessionsSlice'
import { useAppDispatch } from '../../store/hooks'

// User interface
import { Button, FieldError, Form, Input, Label, Modal, TextField, toast } from '@heroui/react'

// Misc
import { renameCodingSession } from '../../api/routes/codingSessionRoutes'
import { SESSION_TITLE_MAX_CHARACTERS } from './createSessionFormSchema'

type Props = {
  state: UseOverlayStateReturn
  session: CodingSession | null
}

// A single field, so plain state instead of a form library.
export function RenameSessionModal(props: Props) {
  const { t } = useTranslation([ 'coding', 'common' ])
  const dispatch = useAppDispatch()
  const [ title, setTitle ] = useState(props.session?.title ?? '')
  const [ isSaving, setIsSaving ] = useState(false)

  const trimmedTitle = title.trim()
  const error = trimmedTitle.length > SESSION_TITLE_MAX_CHARACTERS
    ? t('rename.errors.titleTooLong')
    : null
  const canSave = Boolean(trimmedTitle) && !error && !isSaving

  async function save() {
    if (!props.session || !canSave) {
      return
    }

    setIsSaving(true)
    try {
      const response = await renameCodingSession(props.session.id, trimmedTitle)
      dispatch(codingSessionUpserted(response.session))
      toast.success(t('toasts.renamed'))
      props.state.close()
    }
    catch (saveError) {
      console.debug('RenameSessionModal failed to rename the session', { error: saveError })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsSaving(false)
    }
  }

  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('rename.title')
          }</Modal.Heading>
        </Modal.Header>
        <Form
          onSubmit={(event) => {
            event.preventDefault()
            save()
          }}
          validationBehavior='aria'
        >
          <Modal.Body className='mt-2'>
            <TextField
              autoFocus
              isRequired
              isInvalid={Boolean(error)}
              value={title}
              onChange={setTitle}
            >
              <Label>{t('rename.label')}</Label>
              <Input />
              <FieldError>{error}</FieldError>
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              type='submit'
              isDisabled={!canSave}
              isPending={isSaving}
            >
              <span>{t('common:actions.save')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
