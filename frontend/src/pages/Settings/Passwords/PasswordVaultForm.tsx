// Copyright © 2026 Jalapeno Labs

import type { PasswordVault } from '../../../api/routes/passwordVaultRoutes'
import type { PasswordVaultFormInput, PasswordVaultFormValues } from './passwordVaultFormSchema'

// Core
import { useMemo, useState } from 'react'
import { useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../../store/hooks'
import { passwordVaultLoaded } from '../../../store/passwordVaultSlice'

// User interface
import {
  Alert,
  Button,
  Description,
  FieldError,
  Form,
  Input,
  Label,
  ListBox,
  Select,
  TextField,
  toast,
} from '@heroui/react'
import { SetupChecklist } from '../../../components/SetupChecklist'

// Utility
import { zodResolver } from '@hookform/resolvers/zod'
import { HTTPError } from 'ky'

// Misc
import { getApiErrorMessage } from '../../../api/errors'
import { connectPasswordVault, PASSWORD_SERVERS } from '../../../api/routes/passwordVaultRoutes'
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
// stores anything, so a refusal comes back in Bitwarden's own words and is shown above the
// form. Secrets are write-only: when changing a vault, a blank field keeps the stored one.
export function PasswordVaultForm(props: Props) {
  const { t } = useTranslation([ 'passwords', 'common' ])
  const dispatch = useAppDispatch()
  const stored = props.vault
  const [ refusal, setRefusal ] = useState<string | null>(null)

  const resolver = useMemo(
    () => zodResolver(createPasswordVaultFormSchema(t, stored)),
    [ t, stored ],
  )

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

  const onSubmit = form.handleSubmit(async (values) => {
    setRefusal(null)
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
      toast.success(t('toasts.connected', { email: response.vault.accountEmail }))
      props.onSaved()
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      // Bitwarden's refusal, or the CLI failing to reach the server, names what to fix.
      if (error instanceof HTTPError && [ 400, 502 ].includes(error.response.status) && message) {
        setRefusal(message)
        return
      }
      console.debug('PasswordVaultForm failed to connect the vault', { error })
      toast.danger(t('common:errors.unexpected'), { description: message ?? undefined })
    }
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

      {refusal && <Alert status='danger'>
        <Alert.Indicator />
        <Alert.Content>
          <Alert.Title>{t('form.refusedTitle')}</Alert.Title>
          <Alert.Description>{refusal}</Alert.Description>
        </Alert.Content>
      </Alert>}

      {/* Server */}
      <Select
        isRequired
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
        <Description>{t('form.serverHint')}</Description>
      </Select>

      {/* Server URL, for a self-hosted server */}
      {server === 'self-hosted' && <TextField
        isRequired
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
        autoFocus
        isRequired
        isInvalid={Boolean(errors.clientId)}
        autoComplete='off'
        value={values.clientId}
        onChange={(value) => form.setValue('clientId', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.clientId')}</Label>
        <Input placeholder='user.…' />
        <Description>{t('form.clientIdHint')}</Description>
        <FieldError>{errors.clientId?.message}</FieldError>
      </TextField>

      {/* client_secret */}
      <TextField
        isRequired={!stored}
        isInvalid={Boolean(errors.clientSecret)}
        type='password'
        autoComplete='off'
        value={values.clientSecret}
        onChange={(value) => form.setValue('clientSecret', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.clientSecret')}</Label>
        <Input />
        {stored && <Description>{t('form.clientSecretKeep')}</Description>}
        <FieldError>{errors.clientSecret?.message}</FieldError>
      </TextField>

      {/* Master password */}
      <TextField
        isRequired={!stored}
        isInvalid={Boolean(errors.masterPassword)}
        type='password'
        autoComplete='off'
        value={values.masterPassword}
        onChange={(value) => form.setValue('masterPassword', value, { shouldDirty: true, shouldValidate: true })}
      >
        <Label>{t('form.masterPassword')}</Label>
        <Input />
        <Description>{
          stored
            ? t('form.masterPasswordKeep')
            : t('form.masterPasswordHint')
        }</Description>
        <FieldError>{errors.masterPassword?.message}</FieldError>
      </TextField>

      <p className='text-sm opacity-70'>{t('form.checkedHint')}</p>

      <div className='mt-2 flex justify-end gap-2'>
        {props.onCancel && <Button variant='tertiary' onPress={props.onCancel}>
          <span>{t('common:actions.cancel')}</span>
        </Button>}
        <Button
          type='submit'
          isPending={form.formState.isSubmitting}
        >
          <span>{t('actions.connect')}</span>
        </Button>
      </div>
    </Form>

    <SetupChecklist
      title={t('setup.title')}
      description={t('setup.description')}
      steps={setupStepKeys.map((key) => ({ id: key, text: t(key) }))}
    />
  </div>
}
