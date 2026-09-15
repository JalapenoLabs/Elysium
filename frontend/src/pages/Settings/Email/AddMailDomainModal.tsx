// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { MailDomainFormValues } from './mailFormSchemas'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailDomainUpserted } from '../../../store/mailDomainsSlice'

// User interface
import {
  Button,
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
import { createMailDomain } from '../../../api/routes/mailRoutes'
import { createMailDomainFormSchema } from './mailFormSchemas'

type Props = {
  state: UseOverlayStateReturn
}

export function AddMailDomainModal(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()

  const resolver = useMemo(
    () => zodResolver(createMailDomainFormSchema(t)),
    [ t ],
  )

  const form = useForm<MailDomainFormValues>({
    resolver,
    defaultValues: {
      name: '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = await createMailDomain(values.name)
      dispatch(mailDomainUpserted(response.domain))
      toast.success(t('toasts.domainAdded', { name: response.domain.name }))
      props.state.close()
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('domainForm.errors.taken') })
        return
      }

      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('AddMailDomainModal failed to add the domain', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
    }
  })

  const errors = form.formState.errors
  const [ name ] = useWatch({
    control: form.control,
    name: [ 'name' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('domainForm.title')
          }</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{
            t('domainForm.description')
          }</p>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            <TextField
              autoFocus
              isRequired
              autoComplete='off'
              isInvalid={Boolean(errors.name)}
              value={name}
              onChange={(value) => form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('domainForm.name')}</Label>
              <Input placeholder='example.org' />
              <FieldError>{errors.name?.message}</FieldError>
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
              <span>{t('domainForm.submit')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
