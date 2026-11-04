// Copyright © 2026 Jalapeno Labs

import type { MailDomainFormValues } from './mailFormSchemas'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { mailDomainUpserted } from '../../../store/mailDomainsSlice'

// User interface
import { FieldError, Form, Input, Label, TextField } from '@heroui/react'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getUpstreamErrorMessage } from '../../../api/errors'
import { createMailDomain } from '../../../api/routes/mailRoutes'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { getTabUrl, UrlTree } from '../../../urls'
import { createMailDomainFormSchema } from './mailFormSchemas'
import { toast } from '../../../toast/toast'

// Adds a domain to the mail server, then goes back to Email, which lists it with its DNS records.
export function MailDomainForm() {
  const { t } = useTranslation([ 'email', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()

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

  const save = saveForm(form, async (values) => {
    try {
      const response = await createMailDomain(values.name)
      dispatch(mailDomainUpserted(response.domain))
      toast.success(t('toasts.domainAdded', { name: response.domain.name }))
      void navigate(getTabUrl(UrlTree.settingsEmail, 'domains'))
      return true
    }
    catch (error) {
      if (error instanceof HTTPError && error.response.status === 409) {
        form.setError('name', { message: t('domainForm.errors.taken') })
        return false
      }

      const message = getUpstreamErrorMessage(error)
      if (!message) {
        console.debug('MailDomainForm failed to add the domain', { error })
      }
      toast.danger(message ?? t('common:errors.unexpected'))
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    isDirty: form.formState.isDirty,
    label: t('common:actions.create'),
  })

  const errors = form.formState.errors
  const name = useWatch({ control: form.control, name: 'name' })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='flex max-w-xl flex-col gap-4'>
    <TextField
      id='mail-domain-name'
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

    <PageCreateFooter />
  </Form>
}
