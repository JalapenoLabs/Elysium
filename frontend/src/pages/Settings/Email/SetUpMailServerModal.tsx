// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { SetUpMailServerFormValues } from './mailFormSchemas'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useSWRConfig } from 'swr'

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
import { setUpMailServer } from '../../../api/routes/mailRoutes'
import { createSetUpMailServerFormSchema } from './mailFormSchemas'

type Props = {
  state: UseOverlayStateReturn
}

// Hands the bundled mail server's one-time password to the API, which finishes the
// server's setup and keeps the permanent administrator it is issued.
export function SetUpMailServerModal(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const { mutate } = useSWRConfig()

  const resolver = useMemo(
    () => zodResolver(createSetUpMailServerFormSchema(t)),
    [ t ],
  )

  const form = useForm<SetUpMailServerFormValues>({
    resolver,
    defaultValues: {
      domain: 'elysium.local',
      bootstrapPassword: '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = await setUpMailServer(values)
      await mutate('v1/mail/capabilities')
      toast.success(t('toasts.serverSetUp', { domain: response.server.domain ?? values.domain }))
      props.state.close()
    }
    catch (error) {
      // The API answers 400 only when Stalwart refused the temporary password.
      if (error instanceof HTTPError && error.response.status === 400) {
        form.setError('bootstrapPassword', { message: t('setupForm.errors.passwordRejected') })
        return
      }

      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('SetUpMailServerModal failed to set up the mail server', { error })
      }
      toast.danger(t('toasts.serverSetupFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  })

  const errors = form.formState.errors
  const [ domain, bootstrapPassword ] = useWatch({
    control: form.control,
    name: [ 'domain', 'bootstrapPassword' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('setupForm.title')
          }</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{
            t('setupForm.description')
          }</p>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            {/* Domain */}
            <TextField
              autoFocus
              isRequired
              autoComplete='off'
              isInvalid={Boolean(errors.domain)}
              value={domain}
              onChange={(value) => form.setValue('domain', value, { shouldDirty: true, shouldValidate: true })}
            >
              <Label>{t('setupForm.domain')}</Label>
              <Input />
              <Description>{t('setupForm.domainHint')}</Description>
              <FieldError>{errors.domain?.message}</FieldError>
            </TextField>

            {/* Where the temporary password is. Outside the field, whose description
                gives way to the error message, because it matters most after a rejection. */}
            <div className='text-xs'>
              <p className='opacity-70'>{
                t('setupForm.bootstrapPasswordHint')
              }</p>
              <code className='mt-2 block overflow-x-auto rounded-md bg-default px-3 py-2'>{
                'docker logs elysium-stalwart'
              }</code>
            </div>

            {/* Temporary password */}
            <TextField
              isRequired
              type='password'
              autoComplete='off'
              isInvalid={Boolean(errors.bootstrapPassword)}
              value={bootstrapPassword}
              onChange={(value) => form.setValue(
                'bootstrapPassword',
                value,
                { shouldDirty: true, shouldValidate: true },
              )}
            >
              <Label>{t('setupForm.bootstrapPassword')}</Label>
              <Input />
              <FieldError>{errors.bootstrapPassword?.message}</FieldError>
            </TextField>

            <p className='text-xs opacity-70'>{
              t('setupForm.restartHint')
            }</p>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              type='submit'
              isPending={form.formState.isSubmitting}
            >
              <span>{t('setupForm.submit')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
