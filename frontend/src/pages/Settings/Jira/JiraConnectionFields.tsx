// Copyright © 2026 Jalapeno Labs

import type { UseFormReturn } from 'react-hook-form'
import type { JiraFormInput, JiraFormValues } from './jiraFormSchema'

// Core
import { useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// User interface
import { Description, FieldError, Input, Label, TextField } from '@heroui/react'
import { SecretInput } from '../../../components/SecretInput'

// Misc
import { useInitialFocus } from '../../../hooks/useInitialFocus'

type Props = {
  form: UseFormReturn<JiraFormInput, unknown, JiraFormValues>
  // Whether a blank token field keeps the stored one, which it does for as long as the
  // site and the account it was created for are unchanged.
  keepsStoredToken: boolean
  // True once a credential exists, so the token field can explain what blank means.
  isEditing: boolean
}

// Who Elysium signs in to Jira as, and where: the fields the add and edit forms share.
// The token is write-only and never leaves this field for anywhere but the request.
export function JiraConnectionFields(props: Props) {
  const { t } = useTranslation('jira')
  const errors = props.form.formState.errors
  // useWatch subscribes per field and, unlike form.watch, is safe for the React Compiler.
  const values = useWatch({ control: props.form.control })
  const nameRef = useInitialFocus()

  let tokenHint = t('form.tokenHint')
  if (props.keepsStoredToken) {
    tokenHint = t('form.tokenKeep')
  }
  else if (props.isEditing) {
    tokenHint = t('form.tokenRetype')
  }

  return <>
    {/* Name */}
    <TextField
      isRequired
      id='jira-name'
      autoComplete='off'
      isInvalid={Boolean(errors.name)}
      value={values.name}
      onChange={(value) => props.form.setValue('name', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.name')}</Label>
      <Input ref={nameRef} />
      <Description>{t('form.nameHint')}</Description>
      <FieldError>{errors.name?.message}</FieldError>
    </TextField>

    {/* Site URL */}
    <TextField
      isRequired
      id='jira-site-url'
      isInvalid={Boolean(errors.siteUrl)}
      type='url'
      autoComplete='off'
      value={values.siteUrl}
      onChange={(value) => props.form.setValue('siteUrl', value, { shouldDirty: true, shouldValidate: true })}
    >
      <Label>{t('form.siteUrl')}</Label>
      <Input placeholder='https://acme.atlassian.net' />
      <Description>{t('form.siteUrlHint')}</Description>
      <FieldError>{errors.siteUrl?.message}</FieldError>
    </TextField>

    {/* Account email */}
    <TextField
      isRequired
      id='jira-account-email'
      isInvalid={Boolean(errors.accountEmail)}
      type='email'
      autoComplete='off'
      value={values.accountEmail}
      onChange={(value) => props.form.setValue('accountEmail', value, {
        shouldDirty: true,
        shouldValidate: true,
      })}
    >
      <Label>{t('form.accountEmail')}</Label>
      <Input />
      <Description>{t('form.accountEmailHint')}</Description>
      <FieldError>{errors.accountEmail?.message}</FieldError>
    </TextField>

    {/* API token */}
    <SecretInput
      id='jira-token'
      isRequired={!props.keepsStoredToken}
      isMonospace
      label={t('form.token')}
      description={tokenHint}
      errorMessage={errors.token?.message}
      value={values.token ?? ''}
      onChange={(value) => props.form.setValue('token', value, { shouldDirty: true, shouldValidate: true })}
    />
  </>
}
