// Copyright © 2026 Jalapeno Labs

import type { PasswordVault } from '../../../api/routes/passwordVaultRoutes'
import type { PasswordVaultFormInput, PasswordVaultFormValues } from './passwordVaultFormSchema'

// Core
import { useMemo } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { passwordVaultLoaded } from '../../../store/passwordVaultSlice'

// User interface
import {
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  Select,
  TextField,
} from '@heroui/react'
import { SecretInput } from '../../../components/SecretInput'
import { SetupChecklist } from '../../../components/SetupChecklist'
import { PageCreateFooter } from '../../../layout/page/PageCreateFooter'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { useFormDraft } from '../../../drafts/useFormDraft'
import { getApiErrorMessage } from '../../../api/errors'
import { connectPasswordVault, PASSWORD_SERVERS } from '../../../api/routes/passwordVaultRoutes'
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { saveForm, usePageSave } from '../../../layout/page/pageSave'
import { toast } from '../../../toast/toast'
import { createPasswordVaultFormSchema } from './passwordVaultFormSchema'
import { serverLabelKeys, setupStepKeys } from './passwordsPresentation'

type Props = {
  // The vault whose credentials are being changed, or null to connect one.
  vault: PasswordVault | null
  onSaved: () => void
  // Null when there is nothing to go back to.
  onCancel: (() => void) | null
}

// Connects the vault, or changes its credentials. The API signs in and unlocks before it
// stores anything, so a refusal comes back in Bitwarden's own words, shown in a toast that
// stays until dismissed. Secrets are write-only: when changing a vault, a blank field keeps the stored one.
// Connected from the page's Save, in the header; connecting the first vault also has it below
// the form, beside Back, as every page that creates something does.
export function PasswordVaultForm(props: Props) {
  const { t } = useTranslation([ 'passwords', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.vault

  const resolver = useMemo(
    () => zodResolver(createPasswordVaultFormSchema(t, stored)),
    [ t, stored ],
  )

  const clientIdRef = useInitialFocus()
  const form = useForm<PasswordVaultFormInput, unknown, PasswordVaultFormValues>({
    resolver,
    defaultValues: {
      server: stored?.server ?? 'bitwarden-us',
      serverUrl: stored?.serverUrl ?? '',
      clientId: stored?.clientId ?? '',
      clientSecret: '',
      masterPassword: '',
    },
  })
  // A vault being connected keeps where it is for the tab until it connects, never its keys.
  const forgetDraft = useFormDraft(form, {
    name: 'password-vault',
    fields: [ 'serverUrl' ],
    choices: [ 'server' ],
    isEnabled: !stored,
  })

  const save = saveForm(form, async (values) => {
    try {
      const response = await connectPasswordVault({
        server: values.server,
        serverUrl: values.server === 'self-hosted'
          ? values.serverUrl
          : undefined,
        clientId: values.clientId,
        // Blank means "keep the stored one", so a secret is only sent when typed.
        clientSecret: values.clientSecret || undefined,
        masterPassword: values.masterPassword || undefined,
      })
      dispatch(passwordVaultLoaded({ vault: response.vault, status: response.status }))
      forgetDraft()
      toast.success(t('toasts.connected', { email: response.vault.accountEmail }))
      props.onSaved()
      return true
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      // Bitwarden's refusal, or the CLI failing to reach the server, names what to fix.
      if (error instanceof HTTPError && [ 400, 502 ].includes(error.response.status) && message) {
        toast.danger(t('form.refusedTitle'), { description: message })
        return false
      }
      console.debug('PasswordVaultForm failed to connect the vault', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
      return false
    }
  })

  const onSubmit = usePageSave({
    save,
    onDiscard: forgetDraft,
    isDirty: form.formState.isDirty,
    label: t('actions.connect'),
  })

  const errors = form.formState.errors
  const values = useWatch({ control: form.control })
  const server = values.server ?? 'bitwarden-us'

  return <div className='grid grid-cols-1 items-start gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,24rem)]'>
    <Form onSubmit={onSubmit} validationBehavior='aria' className='flex flex-col gap-4'>
      <h2 className='text-xl font-semibold'>{
        stored
          ? t('form.editHeading')
          : t('form.heading')
      }</h2>

      {/* Server */}
      <Select
        isRequired
        id='password-vault-server'
        value={server}
        onChange={(key) => {
          const selected = PASSWORD_SERVERS.find((candidate) => candidate === key)
          if (!selected) {
            console.debug('PasswordVaultForm ignored an unknown server', { key })
            return
          }
          form.setValue('server', selected, { shouldDirty: true, shouldValidate: true })
        }}
      >
        <Label>{t('form.server')}</Label>
        <Select.Trigger>
          <Select.Value />
          <Select.Indicator />
        </Select.Trigger>
        <Select.Popover>
          <ListBox>{
            PASSWORD_SERVERS.map((candidate) => <ListBox.Item
              key={candidate}
              id={candidate}
              textValue={t(serverLabelKeys[candidate])}
            >
              {t(serverLabelKeys[candidate])}
              <ListBox.ItemIndicator />
            </ListBox.Item>)
          }</ListBox>
        </Select.Popover>
      </Select>

      {/* Server URL, for a self-hosted server */}
      {server === 'self-hosted' && <TextField
        isRequired
        id='password-vault-server-url'
        autoComplete='url'
        isInvalid={Boolean(errors.serverUrl)}
        type='url'
        value={values.serverUrl}
        onChange={(value) => form.setValue('serverUrl', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.serverUrl')}</Label>
        <Input placeholder='https://vault.example.com' />
        <Description>{t('form.serverUrlHint')}</Description>
        <FieldError>{errors.serverUrl?.message}</FieldError>
      </TextField>}

      {/* client_id */}
      <TextField
        isRequired
        id='password-vault-client-id'
        isInvalid={Boolean(errors.clientId)}
        autoComplete='off'
        value={values.clientId}
        onChange={(value) => form.setValue('clientId', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.clientId')}</Label>
        <Input ref={clientIdRef} placeholder='user.…' />
        <Description>{t('form.clientIdHint')}</Description>
        <FieldError>{errors.clientId?.message}</FieldError>
      </TextField>

      {/* client_secret */}
      <SecretInput
        id='password-vault-client-secret'
        isRequired={!stored}
        isMonospace
        label={t('form.clientSecret')}
        description={stored && t('form.clientSecretKeep')}
        errorMessage={errors.clientSecret?.message}
        value={values.clientSecret ?? ''}
        onChange={(value) => form.setValue('clientSecret', value, { shouldDirty: true, shouldValidate: true })}
      />

      {/* Master password */}
      <SecretInput
        id='password-vault-master-password'
        isRequired={!stored}
        label={t('form.masterPassword')}
        description={stored && t('form.masterPasswordKeep')}
        errorMessage={errors.masterPassword?.message}
        value={values.masterPassword ?? ''}
        onChange={(value) => form.setValue('masterPassword', value, { shouldDirty: true, shouldValidate: true })}
      />

      {!stored && <PageCreateFooter />}
      {props.onCancel && <div className='mt-2 flex justify-end'>
        <Button variant='tertiary' onPress={props.onCancel}>
          <span>{t('common:actions.cancel')}</span>
        </Button>
      </div>}
    </Form>

    <SetupChecklist
      id='password-vault-setup'
      title={t('setup.title')}
      steps={setupStepKeys.map((key) => ({ id: key, text: t(key) }))}
    />
  </div>
}
