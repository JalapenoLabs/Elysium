// Copyright © 2026 Jalapeno Labs

import type { MailServerFormValues } from './mailFormSchemas'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailServerUpdated } from '../../../store/mailServerSlice'

// User interface
import { Description, FieldError, Form, Input, Label, TextField } from '@heroui/react'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'

// Misc
import { createMailServer } from '../../../api/routes/mailRoutes'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { UrlTree } from '../../../urls'
import { createMailServerFormSchema } from './mailFormSchemas'
import { toast } from '../../../toast/toast'

// Asks for the server's first domain and hostname, then goes back to Email: creation runs in
// the background, and Email follows each step the event stream reports.
export function MailServerForm() {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
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

  const save = saveForm(form, async (values) => {
    try {
      const response = await createMailServer(values)
      dispatch(mailServerUpdated(response.server))
      void navigate(UrlTree.settingsEmail)
      return true
    }
    catch (error) {
      console.debug('MailServerForm failed to start creating the mail server', { error })
      toast.danger(t('toasts.serverCreateFailed'), {
        description: t('common:errors.unexpected'),
      })
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    isDirty: form.formState.isDirty,
    label: t('common:actions.create'),
  })

  const errors = form.formState.errors
  const [ domain, hostname ] = useWatch({
    control: form.control,
    name: [ 'domain', 'hostname' ],
  })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex max-w-xl flex-col gap-4'>
    {/* First domain */}
    <TextField
      id='mail-server-domain'
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
      id='mail-server-hostname'
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

    <PageCreateFooter />
  </Form>
}
