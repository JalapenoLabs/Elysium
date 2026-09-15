// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { MailAccount } from '../../../api/routes/mailRoutes'
import type { SenderNameFormValues } from './mailFormSchemas'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailAccountUpserted } from '../../../store/mailAccountsSlice'

// User interface
import { Button, Description, FieldError, Form, Input, Label, Modal, TextField, toast } from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { updateMailAccount } from '../../../api/routes/mailRoutes'
import { createSenderNameFormSchema } from './mailFormSchemas'

type Props = {
  state: UseOverlayStateReturn
  account: MailAccount | null
}

// The one thing about a mailbox worth editing: the name recipients see beside it.
export function SenderNameModal(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()

  const resolver = useMemo(
    () => zodResolver(createSenderNameFormSchema(t)),
    [ t ],
  )

  const form = useForm<SenderNameFormValues>({
    resolver,
    defaultValues: {
      displayName: props.account?.displayName ?? '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    if (!props.account) {
      console.debug('SenderNameModal submitted with no account selected')
      return
    }

    try {
      const response = await updateMailAccount(props.account.id, { displayName: values.displayName })
      dispatch(mailAccountUpserted(response.account))
      toast.success(t('toasts.updated', { address: response.account.address }))
      props.state.close()
    }
    catch (error) {
      console.debug('SenderNameModal failed to save the sender name', { error })
      toast.danger(t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  const displayName = useWatch({ control: form.control, name: 'displayName' })

  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('renameForm.title')
          }</Modal.Heading>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2'>
            <TextField
              autoFocus
              isInvalid={Boolean(errors.displayName)}
              value={displayName}
              onChange={(value) => form.setValue('displayName', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('renameForm.displayName')}</Label>
              <Input />
              <Description>{
                t('renameForm.hint', { address: props.account?.address ?? '' })
              }</Description>
              <FieldError>{errors.displayName?.message}</FieldError>
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              type='submit'
              isPending={form.formState.isSubmitting}
            >
              <span>{t('common:actions.save')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
