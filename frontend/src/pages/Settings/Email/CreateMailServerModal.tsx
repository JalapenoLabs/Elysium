// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { MailServerFormValues } from './mailFormSchemas'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailServerUpdated } from '../../../store/mailServerSlice'

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

// Misc
import { createMailServer } from '../../../api/routes/mailRoutes'
import { createMailServerFormSchema } from './mailFormSchemas'

type Props = {
  state: UseOverlayStateReturn
}

// Asks for the server's first domain and hostname, then closes: creation runs in the
// background and reports each step through the event stream.
export function CreateMailServerModal(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  // The hostname follows the domain as `mail.<domain>` until it is edited by hand.
  const [ isHostnameEdited, setIsHostnameEdited ] = useState(false)

  const resolver = useMemo(
    () => zodResolver(createMailServerFormSchema(t)),
    [ t ],
  )

  const form = useForm<MailServerFormValues>({
    resolver,
    defaultValues: {
      domain: '',
      hostname: '',
    },
  })

  const onSubmit = form.handleSubmit(async (values) => {
    try {
      const response = await createMailServer(values)
      dispatch(mailServerUpdated(response.server))
      props.state.close()
    }
    catch (error) {
      console.debug('CreateMailServerModal failed to start creating the mail server', { error })
      toast.danger(t('toasts.serverCreateFailed'), {
        description: t('common:errors.unexpected'),
      })
    }
  })

  const errors = form.formState.errors
  const [ domain, hostname ] = useWatch({
    control: form.control,
    name: [ 'domain', 'hostname' ],
  })

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-lg'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{
            t('serverForm.title')
          }</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{
            t('serverForm.description')
          }</p>
        </Modal.Header>
        <Form onSubmit={onSubmit} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            {/* First domain */}
            <TextField
              autoFocus
              isRequired
              autoComplete='off'
              isInvalid={Boolean(errors.domain)}
              value={domain}
              onChange={(value) => {
                form.setValue('domain', value, { shouldDirty: true, shouldValidate: true })
                if (!isHostnameEdited) {
                  const suggestion = value.trim()
                    ? `mail.${value.trim().toLowerCase()}`
                    : ''
                  form.setValue('hostname', suggestion, { shouldValidate: Boolean(suggestion) })
                }
              }}
            >
              <Label>{t('serverForm.domain')}</Label>
              <Input placeholder='example.com' />
              <Description>{t('serverForm.domainHint')}</Description>
              <FieldError>{errors.domain?.message}</FieldError>
            </TextField>

            {/* Server hostname */}
            <TextField
              isRequired
              autoComplete='off'
              isInvalid={Boolean(errors.hostname)}
              value={hostname}
              onChange={(value) => {
                setIsHostnameEdited(true)
                form.setValue('hostname', value, { shouldDirty: true, shouldValidate: true })
              }}
            >
              <Label>{t('serverForm.hostname')}</Label>
              <Input placeholder='mail.example.com' />
              <Description>{t('serverForm.hostnameHint')}</Description>
              <FieldError>{errors.hostname?.message}</FieldError>
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
              <span>{t('serverForm.submit')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
