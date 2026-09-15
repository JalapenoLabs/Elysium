// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { CreateMailboxFormValues } from './mailFormSchemas'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailAccountUpserted } from '../../../store/mailAccountsSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  Modal,
  TextField,
  toast,
} from '@heroui/react'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { createMailbox } from '../../../api/routes/mailRoutes'
import { createMailboxFormSchema } from './mailFormSchemas'

type Props = {
  state: UseOverlayStateReturn
  // The domain the mail server was set up with, offered first.
  defaultDomain: string
}

export function CreateMailboxModal(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()

  const resolver = useMemo(
    () => zodResolver(createMailboxFormSchema(t)),
    [ t ],
  )

  const form = useForm<CreateMailboxFormValues>({
    resolver,
    defaultValues: {
      localPart: '',
      domain: props.defaultDomain,
      displayName: '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = await createMailbox(values)
      dispatch(mailAccountUpserted(response.account))
      toast.success(t('toasts.created', { address: response.account.address }))
      props.state.close()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('localPart', { message: t('createForm.errors.addressTaken') })
        return
      }

      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('CreateMailboxModal failed to create the mailbox', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const [ localPart, domain, displayName ] = useWatch({
    control: form.control,
    name: [ 'localPart', 'domain', 'displayName' ],
  })
  const address = `${localPart.trim() || '…'}@${domain.trim() || '…'}`.toLowerCase()

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('createForm.title')
          }</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{
            t('createForm.description')
          }</p>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            <div className='grid grid-cols-1 gap-4 sm:grid-cols-2'>
              {/* Local part */}
              <TextField
                autoFocus
                isRequired
                autoComplete='off'
                isInvalid={Boolean(errors.localPart)}
                value={localPart}
                onChange={(value) => form.setValue('localPart', value, { shouldDirty: true, shouldValidate: true })}
              >
                <Label>{t('createForm.localPart')}</Label>
                <Input placeholder='agent' />
                <FieldError>{errors.localPart?.message}</FieldError>
              </TextField>

              {/* Domain */}
              <TextField
                isRequired
                autoComplete='off'
                isInvalid={Boolean(errors.domain)}
                value={domain}
                onChange={(value) => form.setValue('domain', value, { shouldDirty: true, shouldValidate: true })}
              >
                <Label>{t('createForm.domain')}</Label>
                <Input />
                <FieldError>{errors.domain?.message}</FieldError>
              </TextField>
            </div>
            <div className='-mt-2 text-xs'>
              <p className='opacity-70'>{
                t('createForm.domainHint')
              }</p>
              <p className='mt-1 font-medium'>{
                t('createForm.addressPreview', { address })
              }</p>
            </div>

            {/* Sender name */}
            <TextField
              isInvalid={Boolean(errors.displayName)}
              value={displayName}
              onChange={(value) => form.setValue('displayName', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('createForm.displayName')}</Label>
              <Input />
              <Description>{t('createForm.displayNameHint')}</Description>
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
              <span>{t('mailboxes.create')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
